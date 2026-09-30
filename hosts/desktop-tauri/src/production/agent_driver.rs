//! 编排「派活」驱动器（P2）：把任务信封真正交给配置的 Agent 干活。
//!
//! 分工边界（与呈现层同语义）：
//! - [`AgentDriver`] 是 [`OrcCommandHandler`]（service.rs）与具体 Agent 之间的注入边界；
//!   编排推进（create/advance）后由 handler 调 `dispatch` 唤醒当前 Step 的 Agent；
//! - 派活是推进后的**附加动作**：成功/失败都不改变已落库的推进结果；失败返回明确中文错误，
//!   由 handler 落库 blocked（§4.6 不自动重推）并推微信失败提醒。
//!
//! 会话语义取舍（与 adapter trait 现实对齐）：
//! - `AgentAdapter` 只有 `resume`（续聊）；「新会话」是 OpenCode 特有增强：
//!   `AgentAdapter::open` 的默认实现返回 `UnsupportedCapability`，OpenCode 覆写为真开新会话；
//! - `dispatch(open=true)` 时走适配器 [`agentnotify_agent_sdk::AgentAdapter::dispatch_with_options`]：
//!   OpenCode 就绪 → 真开新会话并透传工作目录/模型/无人值守；其他适配器默认实现丢弃选项、
//!   在 open 不支持时降级为 `resume` 续聊同一个稳定 session_id 起步（先让真实 Agent 收到信封干活为验收目标）；
//!   OpenCode 侧 open 失败（插件未连接等）→ 返回明确中文错误，走下游 blocked 路径。
//!
//! session_id 策略见 `OrcCommandHandler`：每 (task, step) 一个稳定会话 id（`task-<id>-step-<n>`），
//! Step 1 试图 open 新会话，后续步 resume 同一 (task, step) 会话。

use std::sync::Arc;

use agentnotify_agent_sdk::{AgentError, AgentRegistry};
use agentnotify_domain::{AgentId, AgentSessionId};
use async_trait::async_trait;

use crate::bridge::error::CommandError;

/// 派活选项（§4 派活透传）：定义在 agent-sdk（适配器 trait 签名共用），此处透传导出。
pub use agentnotify_agent_sdk::DispatchOptions;

/// Agent 未注册（registry 里没有该 id）时的稳定错误码。
pub const ORC_STEP_AGENT_UNREGISTERED: &str = "orc_step_agent_unregistered";
/// 派活失败（resume/open 被适配器拒绝，含插件未连接）时的稳定错误码。
pub const ORC_STEP_DISPATCH_FAILED: &str = "orc_step_dispatch_failed";

/// 编排派活边界：把一个任务信封交给指定 Agent 干活。
///
/// `open=true` 表示新会话（任务首步 Step 1）；`false` 续聊同一 (task, step) 会话。
/// `options` 携带该任务的工作目录、该步模型与无人值守标志（不支持者由适配器默认实现忽略）。
/// 失败返回用户可读的中文错误（微信里看得懂），绝不猜测兜底。
#[async_trait]
pub trait AgentDriver: Send + Sync {
    /// 派活：把 `envelope`（已渲染的任务信封文本）交给 `agent_id` 的 Agent。
    async fn dispatch(
        &self,
        task_id: &str,
        agent_id: &AgentId,
        session_id: &AgentSessionId,
        envelope: &str,
        open: bool,
        options: &DispatchOptions,
    ) -> Result<(), CommandError>;
}

/// 生产派活实现：从 [`AgentRegistry`] 拿适配器，按选项调
/// [`agentnotify_agent_sdk::AgentAdapter::dispatch_with_options`]。
pub struct ProductionAgentDriver {
    registry: Arc<AgentRegistry>,
}

impl ProductionAgentDriver {
    pub fn new(registry: Arc<AgentRegistry>) -> Self {
        Self { registry }
    }
}

#[async_trait]
impl AgentDriver for ProductionAgentDriver {
    async fn dispatch(
        &self,
        _task_id: &str,
        agent_id: &AgentId,
        session_id: &AgentSessionId,
        envelope: &str,
        open: bool,
        options: &DispatchOptions,
    ) -> Result<(), CommandError> {
        let adapter = self.registry.get(agent_id).ok_or_else(|| {
            CommandError::new(
                ORC_STEP_AGENT_UNREGISTERED,
                format!(
                    "无法唤醒 {agent_id}：该 Agent 未启用，请在「Agent 管理」里启用后点「重新发起」"
                ),
            )
        })?;

        adapter
            .dispatch_with_options(session_id, envelope, open, options)
            .await
            .map(|_| ())
            .map_err(|error| dispatch_error(agent_id, &error))
    }
}

/// 派活失败 → 命令错误：保留稳定错误码 + 一句用户可读的原因（哪个 Agent 没唤醒成功，
/// 以及适配器给出的处理办法）；不带任务 ID 与内部实现细节。
fn dispatch_error(agent_id: &AgentId, error: &AgentError) -> CommandError {
    CommandError::new(
        ORC_STEP_DISPATCH_FAILED,
        format!("无法唤醒 {agent_id}：{}", error.message()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentnotify_agent_sdk::{
        AgentAdapter, AgentCapabilities, AgentDescriptor, AgentEventEnvelope, AgentHealth,
        NormalizedAgentEvent, ResumeReceipt,
    };
    use agentnotify_domain::SafeError;
    use std::sync::RwLock;

    /// 记录型假适配器：记下 resume/open 调用，可配置失败与是否支持 open。
    struct FakeAdapter {
        id: &'static str,
        supports_open: bool,
        calls: Arc<RwLock<Vec<(&'static str, String)>>>,
        fail: Arc<RwLock<Option<AgentError>>>,
    }

    impl FakeAdapter {
        fn new(id: &'static str, supports_open: bool) -> Self {
            Self {
                id,
                supports_open,
                calls: Arc::new(RwLock::new(Vec::new())),
                fail: Arc::new(RwLock::new(None)),
            }
        }

        fn calls(&self) -> Vec<(&'static str, String)> {
            self.calls.read().expect("测试锁").clone()
        }

        fn fail_with(&self, error: AgentError) {
            *self.fail.write().expect("测试锁") = Some(error);
        }
    }

    #[async_trait]
    impl AgentAdapter for FakeAdapter {
        fn descriptor(&self) -> AgentDescriptor {
            AgentDescriptor {
                id: AgentId::new(self.id).expect("测试 Agent id 必须合法"),
                display_name: self.id.to_string(),
                description: "测试".to_string(),
                config_schema: Default::default(),
            }
        }

        fn capabilities(&self) -> AgentCapabilities {
            AgentCapabilities::default()
        }

        fn parse_event(
            &self,
            _envelope: AgentEventEnvelope,
        ) -> Result<NormalizedAgentEvent, AgentError> {
            Err(AgentError::InvalidEvent)
        }

        async fn resume(
            &self,
            session_id: &AgentSessionId,
            text: &str,
        ) -> Result<ResumeReceipt, AgentError> {
            self.calls
                .write()
                .expect("测试锁")
                .push(("resume", format!("{}:{text}", session_id.as_str())));
            if let Some(error) = self.fail.read().expect("测试锁").clone() {
                return Err(error);
            }
            Ok(ResumeReceipt {
                session_id: session_id.clone(),
            })
        }

        async fn open(
            &self,
            session_id: &AgentSessionId,
            text: &str,
        ) -> Result<ResumeReceipt, AgentError> {
            self.calls
                .write()
                .expect("测试锁")
                .push(("open", format!("{}:{text}", session_id.as_str())));
            if let Some(error) = self.fail.read().expect("测试锁").clone() {
                return Err(error);
            }
            if !self.supports_open {
                return Err(AgentError::UnsupportedCapability);
            }
            Ok(ResumeReceipt {
                session_id: session_id.clone(),
            })
        }

        async fn inspect(&self) -> AgentHealth {
            AgentHealth::healthy()
        }
    }

    fn session(step: u32) -> AgentSessionId {
        AgentSessionId::new(format!("task-orc-test-step-{step}")).expect("测试会话 id 必须合法")
    }

    /// OpenCode 类适配器：open=true 走真新会话；open=false 走续聊。
    #[tokio::test]
    async fn dispatch_opens_new_session_when_supported_and_requested() {
        let adapter = Arc::new(FakeAdapter::new("opencode", true));
        let mut registry = AgentRegistry::default();
        registry.register(adapter.clone()).expect("注册必须成功");
        let driver = ProductionAgentDriver::new(Arc::new(registry));
        let agent = AgentId::new("opencode").expect("Agent id 必须合法");

        driver
            .dispatch(
                "t-1",
                &agent,
                &session(1),
                "信封1",
                true,
                &DispatchOptions::default(),
            )
            .await
            .expect("open 必须成功");
        driver
            .dispatch(
                "t-1",
                &agent,
                &session(2),
                "信封2",
                false,
                &DispatchOptions::default(),
            )
            .await
            .expect("resume 必须成功");

        // 第一次走真新会话（open），第二次续聊（resume），互不混淆。
        assert_eq!(
            adapter.calls(),
            vec![
                ("open", "task-orc-test-step-1:信封1".into()),
                ("resume", "task-orc-test-step-2:信封2".into()),
            ]
        );
    }

    /// 不支持新会话的适配器（如 Codex）：open=true 先试 open（默认实现零副作用），
    /// 收到 UnsupportedCapability 后降级为 resume 续聊，信封必须送达。
    #[tokio::test]
    async fn dispatch_falls_back_to_resume_when_open_unsupported() {
        let adapter = Arc::new(FakeAdapter::new("codex", false));
        let mut registry = AgentRegistry::default();
        registry.register(adapter.clone()).expect("注册必须成功");
        let driver = ProductionAgentDriver::new(Arc::new(registry));
        let agent = AgentId::new("codex").expect("Agent id 必须合法");

        driver
            .dispatch(
                "t-1",
                &agent,
                &session(1),
                "信封1",
                true,
                &DispatchOptions::default(),
            )
            .await
            .expect("open 不支持时必须降级 resume 成功");

        assert_eq!(
            adapter.calls(),
            vec![
                ("open", "task-orc-test-step-1:信封1".into()),
                ("resume", "task-orc-test-step-1:信封1".into()),
            ]
        );
    }

    /// OpenCode 侧 open 不可达（插件未连接）：返回明确中文错误，不降级、不猜测。
    #[tokio::test]
    async fn dispatch_reports_clear_error_when_open_unavailable() {
        let adapter = Arc::new(FakeAdapter::new("opencode", true));
        adapter.fail_with(AgentError::Unavailable(
            SafeError::new(
                "opencode_plugin_not_ready",
                "OpenCode 插件未连接，请启动 OpenCode 后重试",
            )
            .expect("测试安全错误必须有效"),
        ));
        let mut registry = AgentRegistry::default();
        registry.register(adapter.clone()).expect("注册必须成功");
        let driver = ProductionAgentDriver::new(Arc::new(registry));
        let agent = AgentId::new("opencode").expect("Agent id 必须合法");

        let error = driver
            .dispatch(
                "t-1",
                &agent,
                &session(1),
                "信封1",
                true,
                &DispatchOptions::default(),
            )
            .await
            .expect_err("open 不可达必须报错");
        assert_eq!(error.code(), ORC_STEP_DISPATCH_FAILED);
        assert!(
            error.message().contains("插件未连接"),
            "错误必须写清插件未连接：{}",
            error.message()
        );
        assert!(
            error.message().contains("无法唤醒 opencode"),
            "错误必须写清是哪个 Agent 没唤醒成功：{}",
            error.message()
        );
        assert!(
            !error.message().contains("t-1"),
            "面向用户的原因不得包含任务 ID：{}",
            error.message()
        );
    }

    /// resume 失败（超时/拒绝）：同样明确报错，走下游 blocked。
    #[tokio::test]
    async fn dispatch_reports_clear_error_when_resume_fails() {
        let adapter = Arc::new(FakeAdapter::new("codex", false));
        adapter.fail_with(AgentError::Unknown(
            SafeError::new("codex_queue_unconfirmed", "Codex 未在 30 秒内确认引用回复")
                .expect("测试安全错误必须有效"),
        ));
        let mut registry = AgentRegistry::default();
        registry.register(adapter.clone()).expect("注册必须成功");
        let driver = ProductionAgentDriver::new(Arc::new(registry));
        let agent = AgentId::new("codex").expect("Agent id 必须合法");

        let error = driver
            .dispatch(
                "t-1",
                &agent,
                &session(1),
                "信封1",
                false,
                &DispatchOptions::default(),
            )
            .await
            .expect_err("resume 失败必须报错");
        assert_eq!(error.code(), ORC_STEP_DISPATCH_FAILED);
        assert!(
            error.message().contains("Codex 未在 30 秒内确认"),
            "{}",
            error.message()
        );
    }

    /// Agent 未注册：明确报错（写清哪个 Agent、哪个任务），不猜测。
    #[tokio::test]
    async fn dispatch_reports_clear_error_when_agent_not_registered() {
        let driver = ProductionAgentDriver::new(Arc::new(AgentRegistry::default()));
        let agent = AgentId::new("never-registered").expect("Agent id 必须合法");

        let error = driver
            .dispatch(
                "t-1",
                &agent,
                &session(1),
                "信封1",
                false,
                &DispatchOptions::default(),
            )
            .await
            .expect_err("未注册 Agent 必须报错");
        assert_eq!(error.code(), ORC_STEP_AGENT_UNREGISTERED);
        assert!(
            error.message().contains("never-registered"),
            "{}",
            error.message()
        );
        assert!(
            error.message().contains("Agent 管理"),
            "必须给出处理办法：{}",
            error.message()
        );
        assert!(
            !error.message().contains("t-1"),
            "面向用户的原因不得包含任务 ID：{}",
            error.message()
        );
    }

    /// 派活选项透传：覆写 `dispatch_with_options` 的适配器必须原样收到工作目录/模型/无人值守。
    #[tokio::test]
    async fn dispatch_passes_options_through_to_adapter() {
        #[derive(Default)]
        struct OptionRecording {
            seen: std::sync::Mutex<Vec<DispatchOptions>>,
        }

        #[async_trait]
        impl AgentAdapter for OptionRecording {
            fn descriptor(&self) -> AgentDescriptor {
                AgentDescriptor {
                    id: AgentId::new("recording").expect("测试 Agent id 必须合法"),
                    display_name: "recording".into(),
                    description: "记录派活选项".into(),
                    config_schema: Default::default(),
                }
            }

            fn capabilities(&self) -> AgentCapabilities {
                AgentCapabilities::default()
            }

            fn parse_event(
                &self,
                _envelope: AgentEventEnvelope,
            ) -> Result<NormalizedAgentEvent, AgentError> {
                Err(AgentError::InvalidEvent)
            }

            async fn resume(
                &self,
                session_id: &AgentSessionId,
                _text: &str,
            ) -> Result<ResumeReceipt, AgentError> {
                Ok(ResumeReceipt {
                    session_id: session_id.clone(),
                })
            }

            async fn dispatch_with_options(
                &self,
                session_id: &AgentSessionId,
                _text: &str,
                _open: bool,
                options: &DispatchOptions,
            ) -> Result<ResumeReceipt, AgentError> {
                self.seen.lock().expect("测试锁").push(options.clone());
                Ok(ResumeReceipt {
                    session_id: session_id.clone(),
                })
            }

            async fn inspect(&self) -> AgentHealth {
                AgentHealth::healthy()
            }
        }

        let adapter = Arc::new(OptionRecording::default());
        let mut registry = AgentRegistry::default();
        registry.register(adapter.clone()).expect("注册必须成功");
        let driver = ProductionAgentDriver::new(Arc::new(registry));
        let agent = AgentId::new("recording").expect("Agent id 必须合法");
        let options = DispatchOptions {
            working_dir: Some("D:/Project/demo".into()),
            model: Some("anthropic/claude-sonnet-4-5".into()),
            variant: None,
            unattended: false,
            title: None,
        };

        driver
            .dispatch("t-1", &agent, &session(1), "信封", false, &options)
            .await
            .expect("派活必须成功");

        let seen = adapter.seen.lock().expect("测试锁").clone();
        assert_eq!(seen, vec![options], "选项必须原样透传");
    }
}

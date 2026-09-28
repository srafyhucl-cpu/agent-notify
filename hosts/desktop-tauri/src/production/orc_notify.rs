//! 集群编排 → 微信呈现层（P1-4，§4.6 通知节奏）。
//!
//! 两部分：
//! - 纯规则与渲染（可单测，不依赖 IO）：`should_notify` 按 `notify_mode` 决定某个
//!   Step 转移要不要外发；`render_cluster_message` 生成带前缀/后缀的中文集群消息；
//! - 呈现边界 [`OrcClusterPresenter`]：全局默认通知节奏 + 集群消息外发。
//!   `OrcCommandHandler` 只依赖该 trait（测试注入假实现断言调用次数与文本）；
//!   生产实现 [`ProductionOrcPresenter`] 读 settings 键 `orchestration.notify_mode`，
//!   推送目标复用 `ProductionTargetProvider` 的账号解析（默认账号优先，其次最近会话）。
//!
//! 规则（§4.6）：`final_only` 只推最终汇报（Complete）与人工确认门（WaitConfirm）；
//! `verbose` 每个推进类转移都推；两者都不推提问/信息（Stay）；
//! **失败提醒不过本规则**：`mark_blocked` 一律外发（write 失败 Step/原因，不自动重推）。

use std::sync::Arc;

use agentnotify_application::StatusStore;
use agentnotify_channel_sdk::{ChannelRegistry, OutboundMessage};
use agentnotify_domain::SafeError;
use agentnotify_orchestration::{MessageKind, NotifyMode, StepOutcome, TransitionAction};

use super::orc_handler::KEY_ORCHESTRATION_NOTIFY_MODE;
use super::orc_wechat_route::{channel_error_safe, valid_notice_receipt};
use super::settings::ProductionSettingsStore;
use super::targets::ProductionTargetProvider;

/// 找不到可投递账号时的稳定错误码（日志用）。
const ORC_CLUSTER_NO_TARGET: &str = "orc_cluster_no_target";
/// 渠道适配器不可用（注册表缺失）时的稳定错误码（日志用）。
const ORC_CLUSTER_CHANNEL_UNREACHABLE: &str = "orc_cluster_channel_unavailable";
/// 渠道未确认集群消息回执（投递层失败，不影响任务状态）。
const ORC_CLUSTER_REPLY_UNCONFIRMED: &str = "orc_cluster_reply_unconfirmed";

/// 呈现层边界（P1-4）：`OrcCommandHandler` 经它读取默认节奏并外发集群消息。
///
/// 实现要求：`push` 失败只记录（日志/诊断），不重试、不影响任务状态——编排推进
/// 的结果已落库，推送只是呈现。
#[async_trait::async_trait]
pub trait OrcClusterPresenter: Send + Sync {
    /// 全局默认通知节奏（`orchestration.notify_mode`）：读取失败/缺失/非法 → `final_only` 并告警。
    async fn default_notify_mode(&self) -> NotifyMode;

    /// 外发一条集群消息（已渲染的中文文本，带前缀/后缀）；投递失败只记录。
    async fn push(&self, task_id: &str, text: String);
}

/// 单次 Step 转移是否外发微信（纯规则，§4.6 通知节奏）。
///
/// - `final_only`：只推最终完成（Complete）与人工确认门（WaitConfirm）；
/// - `verbose`：每个推进类转移都推（Advance / Complete / WaitConfirm / BackToWork / Recover）；
/// - `Stay`（提问/信息/重复汇报）两个模式都不推；
/// - 失败提醒不经过本函数：`mark_blocked` 一律外发（调用方直接走失败分支）。
pub fn should_notify(mode: NotifyMode, outcome: &StepOutcome) -> bool {
    match mode {
        NotifyMode::FinalOnly => {
            matches!(
                outcome.action,
                TransitionAction::Complete | TransitionAction::WaitConfirm
            )
        }
        NotifyMode::Verbose => !matches!(outcome.action, TransitionAction::Stay),
    }
}

/// 集群消息渲染（§4.6 R6）：前缀「【集群 <task_id>】」+ 正文 + 后缀「【<task_id> · Step k/N · 状态】」。
///
/// `step` = 消息落点步骤（转移后当前步骤），`state` = 状态中文（微信呈现侧映射）。
pub fn render_cluster_message(
    task_id: &str,
    step: u32,
    total_steps: u32,
    state: &str,
    body: &str,
) -> String {
    format!(
        "【集群 {task_id}】\n{body}\n──────────────\n【{task_id} · Step {step}/{total_steps} · {state}】"
    )
}

/// 推进类转移的中文正文（§4.6 呈现）。`step` = 收到汇报/指令的那一步序号（推进后由调用方换算）。
pub fn progress_body(kind: MessageKind, action: TransitionAction, step: u32) -> String {
    use TransitionAction::{Advance, BackToWork, Complete, Recover, Stay, WaitConfirm};
    match action {
        Advance => format!("Step {step} 汇报完成，任务推进到下一步"),
        WaitConfirm => format!("Step {step} 汇报已收到，等待你的确认"),
        Complete => match kind {
            MessageKind::Confirm => "确认通过，任务已完成".to_owned(),
            _ => "最终汇报已收到，任务已完成".to_owned(),
        },
        BackToWork => format!("已收到指令，Step {step} 重新干活"),
        Recover => format!("已收到恢复指令，Step {step} 重新干活"),
        // Stay 已被 should_notify 过滤，正常不会到达；给出可读文案兜底（不 panic）。
        Stay => format!("Step {step} 状态不变"),
    }
}

/// 失败提醒正文（§4.6 R8）：写清哪一步失败、谁不可用/未送达、需人工处理，不自动重推。
pub fn failure_body(step: u32, reason: &str) -> String {
    format!("Step {step} 失败：{reason}。任务已阻塞，需人工处理（不会自动重推）。")
}

/// 生产呈现实现：默认节奏读 settings；推送目标复用 `ProductionTargetProvider` 的账号解析。
pub struct ProductionOrcPresenter {
    settings: ProductionSettingsStore,
    targets: Arc<ProductionTargetProvider>,
    channels: Arc<ChannelRegistry>,
    /// 推送失败记录（诊断可见，与 P1-3 回执失败记录一致）。
    status: Option<Arc<dyn StatusStore>>,
}

impl ProductionOrcPresenter {
    pub fn new(
        settings: ProductionSettingsStore,
        targets: Arc<ProductionTargetProvider>,
        channels: Arc<ChannelRegistry>,
        status: Option<Arc<dyn StatusStore>>,
    ) -> Self {
        Self {
            settings,
            targets,
            channels,
            status,
        }
    }

    async fn record_send_error(&self, error: SafeError) {
        tracing::warn!(code = error.code(), "集群消息推送失败，不影响任务状态");
        if let Some(status) = &self.status {
            if let Err(store_error) = status.record_error(error).await {
                tracing::warn!(code = store_error.code(), "记录集群消息推送错误失败");
            }
        }
    }
}

#[async_trait::async_trait]
impl OrcClusterPresenter for ProductionOrcPresenter {
    async fn default_notify_mode(&self) -> NotifyMode {
        let entries = match self.settings.store().settings_entries().await {
            Ok(entries) => entries,
            Err(error) => {
                tracing::warn!(
                    %error,
                    "读取 orchestration.notify_mode 失败，按默认 final_only 处理"
                );
                return NotifyMode::FinalOnly;
            }
        };
        let raw = entries
            .get(KEY_ORCHESTRATION_NOTIFY_MODE)
            .and_then(serde_json::Value::as_str);
        let mode = parse_global_notify_mode(raw);
        if mode == NotifyMode::FinalOnly && raw != Some("final_only") {
            tracing::warn!(
                value = ?raw,
                "orchestration.notify_mode 缺失或非法，按默认 final_only 处理"
            );
        }
        mode
    }

    async fn push(&self, task_id: &str, text: String) {
        // 目标账号解析复用 ProductionTargetProvider（显式默认账号优先，其次最近建立会话）。
        let target = match self.targets.first_delivery_target().await {
            Ok(Some(target)) => target,
            Ok(None) => {
                tracing::warn!(
                    task_id,
                    code = ORC_CLUSTER_NO_TARGET,
                    "集群消息没有可投递的微信账号，跳过（任务状态不受影响）"
                );
                return;
            }
            Err(error) => {
                tracing::warn!(
                    task_id,
                    %error,
                    "解析集群消息投递目标失败，跳过（任务状态不受影响）"
                );
                return;
            }
        };

        let Some(channel) = self.channels.get(&target.account.channel_id) else {
            tracing::warn!(
                task_id,
                code = ORC_CLUSTER_CHANNEL_UNREACHABLE,
                "集群消息渠道适配器不可用，跳过（任务状态不受影响）"
            );
            return;
        };
        let capabilities = channel.capabilities();
        // Notification 用途、不带结构化 presentation：按纯文本原样发送（集群消息自带前缀/后缀）。
        let outbound = match OutboundMessage::notification(
            target.conversation_id.clone(),
            text,
            format!("orc-cluster-{task_id}-{}", uuid::Uuid::new_v4()),
        ) {
            Ok(message) => message,
            Err(error) => {
                tracing::warn!(
                    task_id,
                    code = error.code(),
                    "构造集群消息失败，跳过（任务状态不受影响）"
                );
                return;
            }
        };
        match channel.send(target.account, outbound).await {
            Ok(receipt) if valid_notice_receipt(&receipt, &capabilities) => {}
            Ok(_) => {
                self.record_send_error(
                    SafeError::new(ORC_CLUSTER_REPLY_UNCONFIRMED, "渠道未确认集群消息投递")
                        .expect("内置安全错误必须有效"),
                )
                .await;
            }
            Err(error) => self.record_send_error(channel_error_safe(error)).await,
        }
    }
}

/// 解析 `orchestration.notify_mode` 设置取值（纯函数，P1-4）：
/// 缺失或未知取值 → `final_only`（由调用方告警，本函数只回退不猜测）。
fn parse_global_notify_mode(raw: Option<&str>) -> NotifyMode {
    match raw {
        Some("verbose") => NotifyMode::Verbose,
        _ => NotifyMode::FinalOnly,
    }
}

#[cfg(test)]
mod tests {
    use agentnotify_orchestration::{
        NotifyMode, step_machine::StepOutcome, step_machine::StepState,
        step_machine::TransitionAction,
    };

    use super::*;

    fn outcome(action: TransitionAction) -> StepOutcome {
        StepOutcome {
            state: StepState::InProgress,
            action,
        }
    }

    /// final_only 规则矩阵：只推最终完成与人工确认门，中间推进/指令/stay 都不推。
    #[test]
    fn final_only_pushes_only_complete_and_gate() {
        use TransitionAction::*;
        assert!(should_notify(NotifyMode::FinalOnly, &outcome(Complete)));
        assert!(should_notify(NotifyMode::FinalOnly, &outcome(WaitConfirm)));
        assert!(!should_notify(NotifyMode::FinalOnly, &outcome(Advance)));
        assert!(!should_notify(NotifyMode::FinalOnly, &outcome(BackToWork)));
        assert!(!should_notify(NotifyMode::FinalOnly, &outcome(Recover)));
        assert!(!should_notify(NotifyMode::FinalOnly, &outcome(Stay)));
    }

    /// verbose 规则矩阵：每个推进类转移都推，只有 keep-alive（提问/信息）不推。
    #[test]
    fn verbose_pushes_every_transition_except_stay() {
        use TransitionAction::*;
        for action in [Complete, WaitConfirm, Advance, BackToWork, Recover] {
            assert!(
                should_notify(NotifyMode::Verbose, &outcome(action)),
                "verbose 必须推送 {action:?}"
            );
        }
        assert!(!should_notify(NotifyMode::Verbose, &outcome(Stay)));
    }

    /// 渲染：前缀 + 正文 + 后缀（任务 ID、步数 k/N、状态中文）。
    #[test]
    fn render_has_prefix_and_suffix() {
        let text = render_cluster_message(
            "task-1",
            2,
            3,
            "干活中",
            "Step 1 汇报完成，任务推进到下一步",
        );
        assert!(text.contains("【集群 task-1】"), "缺少前缀：{text}");
        assert!(
            text.contains("Step 1 汇报完成，任务推进到下一步"),
            "缺少正文：{text}"
        );
        assert!(
            text.contains("【task-1 · Step 2/3 · 干活中】"),
            "缺少后缀：{text}"
        );
    }

    /// 推进正文：每个转移都有中文说明；完成态区分"汇报到达"与"确认通过"。
    #[test]
    fn progress_bodies_are_readable_chinese() {
        assert_eq!(
            progress_body(MessageKind::Report, TransitionAction::Advance, 1),
            "Step 1 汇报完成，任务推进到下一步"
        );
        assert_eq!(
            progress_body(MessageKind::Report, TransitionAction::WaitConfirm, 4),
            "Step 4 汇报已收到，等待你的确认"
        );
        assert_eq!(
            progress_body(MessageKind::Report, TransitionAction::Complete, 4),
            "最终汇报已收到，任务已完成"
        );
        assert_eq!(
            progress_body(MessageKind::Confirm, TransitionAction::Complete, 4),
            "确认通过，任务已完成"
        );
        assert_eq!(
            progress_body(MessageKind::Instruction, TransitionAction::BackToWork, 2),
            "已收到指令，Step 2 重新干活"
        );
        assert_eq!(
            progress_body(MessageKind::Instruction, TransitionAction::Recover, 2),
            "已收到恢复指令，Step 2 重新干活"
        );
    }

    /// 失败提醒：写清哪一步失败、原因、需人工处理、不自动重推。
    #[test]
    fn failure_body_states_step_reason_and_manual_handling() {
        let text = failure_body(2, "opencode 会话不可用（未登录），消息未送达");
        assert!(text.contains("Step 2 失败"), "{text}");
        assert!(
            text.contains("opencode 会话不可用（未登录），消息未送达"),
            "{text}"
        );
        assert!(text.contains("需人工处理"), "{text}");
        assert!(text.contains("不会自动重推"), "{text}");
    }

    /// 全局默认节奏解析：verbose 生效；缺失/未知取值回退 final_only（不猜测）。
    #[test]
    fn global_default_parse_falls_back_on_missing_or_invalid() {
        assert_eq!(
            parse_global_notify_mode(Some("verbose")),
            NotifyMode::Verbose
        );
        assert_eq!(
            parse_global_notify_mode(Some("final_only")),
            NotifyMode::FinalOnly
        );
        assert_eq!(parse_global_notify_mode(None), NotifyMode::FinalOnly);
        assert_eq!(
            parse_global_notify_mode(Some("noisy")),
            NotifyMode::FinalOnly
        );
    }
}

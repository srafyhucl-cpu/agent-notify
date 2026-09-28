//! Agent 汇报自动回注（§4.4）：监听 agent 事件，把「该 Step 的 Agent 汇报到达」
//! 回注到编排任务推进。
//!
//! 链路：派活时信封送入会话 `task-<task_id>-step-<n>`（见 `AgentDriver` 的 session 策略），
//! Agent 干完活 → OpenCode 插件上报 `session.completed` 事件（sessionId 即该会话 id）→
//! 运行时接纳事件后回调本观察者 → 解析 session id → 复用 [`OrcCommandHandler::report_from_agent`]
//! 推进任务（含通知节奏呈现与下一步派活）。
//!
//! 忽略语义：非 `session.completed`、session id 不匹配编排会话、任务步不一致/非干活中
//! ——全部静默忽略（过期/无关事件），不打扰用户；回注失败只记日志。

use std::sync::Arc;

use agentnotify_agent_sdk::AgentEventEnvelope;
use agentnotify_runtime::AgentEventObserver;

use super::orc_handler::OrcCommandHandler;

/// 编排会话 id 前缀：`task-<task_id>-step-<n>`（与 `AgentDriver` 的 session 策略一致）。
const ORC_SESSION_PREFIX: &str = "task-";
/// 编排会话 id 中的步骤分隔符。
const ORC_SESSION_STEP_MARKER: &str = "-step-";
/// 会话完成事件类型（OpenCode 插件上报 `payload.eventType`）。
const EVENT_SESSION_COMPLETED: &str = "session.completed";

/// 从编排会话 id 解析 `(task_id, step)`；非编排会话返回 `None`。
///
/// task_id 为 UUID（只含十六进制与连字符），`-step-` 中的字母不会是十六进制字符，
/// 因此 `rsplit_once` 不会切错；step 必须是 >=1 的数字。
pub(crate) fn parse_orc_session(session_id: &str) -> Option<(&str, u32)> {
    let rest = session_id.strip_prefix(ORC_SESSION_PREFIX)?;
    let (task_id, step) = rest.rsplit_once(ORC_SESSION_STEP_MARKER)?;
    let step = step.parse::<u32>().ok()?;
    if task_id.is_empty() || step == 0 {
        return None;
    }
    Some((task_id, step))
}

/// 编排汇报观察者：持有命令处理器，把匹配的 session 完成事件回注为任务推进。
pub struct OrcReportObserver {
    handler: Arc<OrcCommandHandler>,
}

impl OrcReportObserver {
    pub fn new(handler: Arc<OrcCommandHandler>) -> Self {
        Self { handler }
    }
}

#[async_trait::async_trait]
impl AgentEventObserver for OrcReportObserver {
    async fn observe(&self, envelope: &AgentEventEnvelope) {
        let payload = &envelope.payload;
        let event_type = payload
            .get("eventType")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        if event_type != EVENT_SESSION_COMPLETED {
            return;
        }
        let Some(session_id) = payload.get("sessionId").and_then(serde_json::Value::as_str) else {
            return;
        };
        let Some((task_id, step)) = parse_orc_session(session_id) else {
            return;
        };
        // 事件正文与显式失败标记（产出记录 / 失败路径用；缺失按成功处理）。
        let body = payload
            .get("body")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        let failed = payload
            .get("failed")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        match self
            .handler
            .report_from_agent(task_id, step, body, failed)
            .await
        {
            Ok(true) => tracing::info!(task_id, step, "Agent 汇报已回注，任务自动推进"),
            Ok(false) => tracing::debug!(task_id, step, "Agent 汇报与任务当前状态不匹配，已忽略"),
            Err(error) => tracing::warn!(
                task_id,
                step,
                code = error.code(),
                "Agent 汇报回注失败：{}",
                error.message()
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_orc_session_matches_expected_shape() {
        let (task_id, step) =
            parse_orc_session("task-8ee6ba0a-14a6-4fbb-81a0-9f0518c6fb94-step-3").unwrap();
        assert_eq!(task_id, "8ee6ba0a-14a6-4fbb-81a0-9f0518c6fb94");
        assert_eq!(step, 3);
    }

    #[test]
    fn parse_orc_session_rejects_foreign_and_malformed() {
        // 普通 OpenCode 会话（非编排）。
        assert!(parse_orc_session("ses_abc123").is_none());
        // 缺 step 段。
        assert!(parse_orc_session("task-8ee6ba0a14a64fbb81a0").is_none());
        // step 非数字。
        assert!(parse_orc_session("task-abc-step-x").is_none());
        // step=0 非法。
        assert!(parse_orc_session("task-abc-step-0").is_none());
        // task_id 为空。
        assert!(parse_orc_session("task--step-1").is_none());
    }
}

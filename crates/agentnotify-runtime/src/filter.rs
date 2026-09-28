//! Agent 事件通知过滤：允许宿主抑制特定事件的通知（不建通知/投递）。
//!
//! 用途（§5 通知规则）：编排会话的原始完成事件不推微信——事件仍需回调
//! [`crate::AgentEventObserver`]（汇报回注、产出记录照常）。
//!
//! 语义：
//! - `allow_notification == true`：按常规链路 ingest（创建通知/投递）；
//! - `allow_notification == false`：跳过 ingest，但仍回调观察者；spool 条目照常 ack
//!   （事件已被接纳处理，不再重复投递）。
//!
//! 实现必须**同步且快速**（在管道/重放热路径上调用），不得 panic。

use std::sync::Arc;

use agentnotify_agent_sdk::AgentEventEnvelope;

/// 通知过滤边界：宿主注入（桌面端用它抑制编排会话的原始完成事件）。
pub trait AgentEventFilter: Send + Sync {
    /// 是否允许为该事件创建通知/投递；`false` = 只走观察者，不推微信。
    fn allow_notification(&self, envelope: &AgentEventEnvelope) -> bool;
}

/// 过滤器句柄（可注入 [`crate::RuntimeConfig`]）。
pub type SharedAgentEventFilter = Arc<dyn AgentEventFilter>;

/// 判定一次事件是否允许创建通知；未注入过滤器 = 全部放行。
pub(crate) fn allow_notification(
    filter: Option<&SharedAgentEventFilter>,
    envelope: &AgentEventEnvelope,
) -> bool {
    match filter {
        Some(filter) => std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            filter.allow_notification(envelope)
        }))
        .unwrap_or_else(|_| {
            tracing::error!("事件通知过滤器发生 panic，已按放行处理（不影响事件消费）");
            true
        }),
        None => true,
    }
}

#[cfg(test)]
mod tests {
    use agentnotify_agent_sdk::AgentEventEnvelope;
    use agentnotify_domain::{AgentId, RequestId};

    use super::*;

    fn envelope() -> AgentEventEnvelope {
        AgentEventEnvelope {
            request_id: RequestId::new("req-filter-test").expect("有效请求标识"),
            agent_id: AgentId::new("opencode").expect("有效 Agent 标识"),
            payload: serde_json::json!({ "eventType": "session.completed" }),
        }
    }

    struct DenyAll;
    impl AgentEventFilter for DenyAll {
        fn allow_notification(&self, _envelope: &AgentEventEnvelope) -> bool {
            false
        }
    }

    struct PanicFilter;
    impl AgentEventFilter for PanicFilter {
        fn allow_notification(&self, _envelope: &AgentEventEnvelope) -> bool {
            panic!("filter boom");
        }
    }

    #[test]
    fn missing_filter_allows_everything() {
        assert!(allow_notification(None, &envelope()));
    }

    #[test]
    fn filter_decision_is_honored() {
        let filter: SharedAgentEventFilter = Arc::new(DenyAll);
        assert!(!allow_notification(Some(&filter), &envelope()));
    }

    #[test]
    fn panic_in_filter_fails_open() {
        let filter: SharedAgentEventFilter = Arc::new(PanicFilter);
        assert!(
            allow_notification(Some(&filter), &envelope()),
            "过滤器 panic 不得阻断事件链路（按放行处理）"
        );
    }
}

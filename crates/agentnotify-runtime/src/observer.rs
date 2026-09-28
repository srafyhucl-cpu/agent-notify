//! Agent 事件观察者：运行时接纳 agent 事件后回调（可选注入）。
//!
//! 用途：编排层用它把「Agent 汇报到达」回注任务推进（§4.4：Step 推进 = 该 Step 的
//! Agent 汇报到达 + 可选人确认门）。观察是**旁路**：失败只记日志，绝不改变
//! 事件消费结果，也不阻塞入口处理。

use std::sync::Arc;

use agentnotify_agent_sdk::AgentEventEnvelope;

/// Agent 事件观察者。实现必须自吞异常（只记日志），不得 panic。
#[async_trait::async_trait]
pub trait AgentEventObserver: Send + Sync {
    /// 事件被运行时接纳后回调（spool 重放的旧事件也会回调，由实现自行按内容判重/忽略）。
    async fn observe(&self, envelope: &AgentEventEnvelope);
}

/// 观察者句柄（可注入 [`crate::RuntimeConfig`]）。
pub type SharedAgentEventObserver = Arc<dyn AgentEventObserver>;

/// 处理一次观察：无观察者时直接返回；有观察者时吞掉其错误并记日志。
pub(crate) async fn notify_observer(
    observer: Option<&SharedAgentEventObserver>,
    envelope: &AgentEventEnvelope,
) {
    let Some(observer) = observer else {
        return;
    };
    let request_id = envelope.request_id.to_string();
    let agent_id = envelope.agent_id.to_string();
    let result = std::panic::AssertUnwindSafe(observer.observe(envelope));
    if futures::FutureExt::catch_unwind(result).await.is_err() {
        tracing::error!(
            %request_id,
            %agent_id,
            "Agent 事件观察者发生 panic，已忽略（不影响事件消费）"
        );
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use agentnotify_agent_sdk::AgentEventEnvelope;
    use agentnotify_domain::{AgentId, RequestId};

    use super::*;

    fn envelope() -> AgentEventEnvelope {
        AgentEventEnvelope {
            request_id: RequestId::new("req-observer-test").expect("有效请求标识"),
            agent_id: AgentId::new("opencode").expect("有效 Agent 标识"),
            payload: serde_json::json!({ "eventType": "session.completed" }),
        }
    }

    struct CountingObserver {
        count: Arc<AtomicUsize>,
    }

    #[async_trait::async_trait]
    impl AgentEventObserver for CountingObserver {
        async fn observe(&self, _envelope: &AgentEventEnvelope) {
            self.count.fetch_add(1, Ordering::SeqCst);
        }
    }

    struct PanicObserver;

    #[async_trait::async_trait]
    impl AgentEventObserver for PanicObserver {
        async fn observe(&self, _envelope: &AgentEventEnvelope) {
            panic!("observer boom");
        }
    }

    #[tokio::test]
    async fn notify_calls_observer_when_present() {
        let count = Arc::new(AtomicUsize::new(0));
        let observer: SharedAgentEventObserver = Arc::new(CountingObserver {
            count: count.clone(),
        });
        notify_observer(Some(&observer), &envelope()).await;
        assert_eq!(count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn notify_without_observer_is_noop() {
        // 无观察者时直接返回，不 panic（保证既有路径零影响）。
        notify_observer(None, &envelope()).await;
    }

    #[tokio::test]
    async fn notify_swallows_observer_panic() {
        // 观察者 panic 被吞掉，不影响事件消费链路。
        let observer: SharedAgentEventObserver = Arc::new(PanicObserver);
        notify_observer(Some(&observer), &envelope()).await;
    }
}

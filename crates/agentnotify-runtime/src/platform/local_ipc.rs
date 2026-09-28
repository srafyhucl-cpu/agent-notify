use std::sync::Arc;

use agentnotify_application::IngestService;
use agentnotify_ingress::{HandlerError, IngressHandler};

use crate::filter::allow_notification;
use crate::observer::notify_observer;
use crate::{SharedAgentEventFilter, SharedAgentEventObserver};

pub(crate) struct RuntimeIngressHandler {
    ingest: Arc<IngestService>,
    /// Agent 事件观察者（可选）：事件接纳后回调（编排汇报回注等），旁路不阻塞消费。
    observer: Option<SharedAgentEventObserver>,
    /// 通知过滤器（可选）：不允许时跳过 ingest（不建通知/投递），但仍回调观察者。
    filter: Option<SharedAgentEventFilter>,
}

impl RuntimeIngressHandler {
    pub(crate) fn new(
        ingest: Arc<IngestService>,
        observer: Option<SharedAgentEventObserver>,
        filter: Option<SharedAgentEventFilter>,
    ) -> Self {
        Self {
            ingest,
            observer,
            filter,
        }
    }
}

#[async_trait::async_trait]
impl IngressHandler for RuntimeIngressHandler {
    async fn handle(
        &self,
        envelope: agentnotify_agent_sdk::AgentEventEnvelope,
    ) -> Result<(), HandlerError> {
        // 过滤：编排会话的原始完成事件不建通知/投递；观察者（汇报回注/产出记录）照常回调。
        if !allow_notification(self.filter.as_ref(), &envelope) {
            tracing::debug!(
                request_id = %envelope.request_id,
                "事件按过滤器跳过 ingest（仍回调观察者）"
            );
            notify_observer(self.observer.as_ref(), &envelope).await;
            return Ok(());
        }
        let accepted = self
            .ingest
            .ingest(envelope.clone())
            .await
            .map(|_| ())
            .map_err(|_| HandlerError::new("ingest_retry", "入口事件暂未接纳，稍后重试"));
        if accepted.is_ok() {
            // 汇报回注等观察在接纳后进行；观察失败只记日志（notify_observer 已兜底）。
            notify_observer(self.observer.as_ref(), &envelope).await;
        }
        accepted
    }
}

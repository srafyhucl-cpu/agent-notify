use std::sync::Arc;

use agentnotify_application::IngestService;
use agentnotify_ingress::{HandlerError, IngressHandler};

use crate::SharedAgentEventObserver;
use crate::observer::notify_observer;

pub(crate) struct RuntimeIngressHandler {
    ingest: Arc<IngestService>,
    /// Agent 事件观察者（可选）：事件接纳后回调（编排汇报回注等），旁路不阻塞消费。
    observer: Option<SharedAgentEventObserver>,
}

impl RuntimeIngressHandler {
    pub(crate) fn new(
        ingest: Arc<IngestService>,
        observer: Option<SharedAgentEventObserver>,
    ) -> Self {
        Self { ingest, observer }
    }
}

#[async_trait::async_trait]
impl IngressHandler for RuntimeIngressHandler {
    async fn handle(
        &self,
        envelope: agentnotify_agent_sdk::AgentEventEnvelope,
    ) -> Result<(), HandlerError> {
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

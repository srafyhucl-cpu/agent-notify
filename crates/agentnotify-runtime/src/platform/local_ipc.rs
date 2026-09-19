use std::sync::Arc;

use agentnotify_application::IngestService;
use agentnotify_ingress::{HandlerError, IngressHandler};

pub(crate) struct RuntimeIngressHandler {
    ingest: Arc<IngestService>,
}

impl RuntimeIngressHandler {
    pub(crate) fn new(ingest: Arc<IngestService>) -> Self {
        Self { ingest }
    }
}

#[async_trait::async_trait]
impl IngressHandler for RuntimeIngressHandler {
    async fn handle(
        &self,
        envelope: agentnotify_agent_sdk::AgentEventEnvelope,
    ) -> Result<(), HandlerError> {
        self.ingest
            .ingest(envelope)
            .await
            .map(|_| ())
            .map_err(|_| HandlerError::new("ingest_retry", "入口事件暂未接纳，稍后重试"))
    }
}

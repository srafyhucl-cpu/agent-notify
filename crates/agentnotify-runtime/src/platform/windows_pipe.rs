use std::sync::Arc;

use agentnotify_application::IngestService;
use agentnotify_ingress::{PipeError, serve};
use tokio::sync::watch;

use crate::{
    ComponentFailure, SharedAgentEventObserver, platform::local_ipc::RuntimeIngressHandler,
};

pub(crate) async fn run_ingress_server(
    ingest: Arc<IngestService>,
    observer: Option<SharedAgentEventObserver>,
    cancel: watch::Receiver<bool>,
) -> Result<(), ComponentFailure> {
    let handler = Arc::new(RuntimeIngressHandler::new(ingest, observer));
    serve(handler, cancel)
        .await
        .map_err(|error| component_failure(&error))
}

fn component_failure(error: &PipeError) -> ComponentFailure {
    ComponentFailure::new(
        error.code(),
        error.message(),
        matches!(error, PipeError::AlreadyRunning),
    )
}

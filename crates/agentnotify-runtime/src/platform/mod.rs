#[cfg(windows)]
mod local_ipc;
#[cfg(windows)]
mod windows_pipe;

#[cfg(windows)]
pub(crate) use windows_pipe::run_ingress_server;

#[cfg(not(windows))]
pub(crate) async fn run_ingress_server(
    _ingest: std::sync::Arc<agentnotify_application::IngestService>,
    _cancel: tokio::sync::watch::Receiver<bool>,
) -> Result<(), crate::ComponentFailure> {
    Ok(())
}

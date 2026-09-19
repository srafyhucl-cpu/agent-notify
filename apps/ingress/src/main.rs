#![cfg_attr(windows, windows_subsystem = "windows")]

use std::{
    io::{self, Read},
    process::ExitCode,
};

use agentnotify_ingress::{
    IngressError, IngressEvent, Spool, SpoolError, SpoolLimits, default_spool_dir,
    protocol::MAX_PROTOCOL_BYTES,
};

#[cfg(windows)]
use agentnotify_ingress::{DEFAULT_CONNECT_TIMEOUT, SubmitResult, pipe_name, submit_with_fallback};

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(IngressExitError::Protocol) => ExitCode::from(2),
        Err(IngressExitError::Spool) => ExitCode::from(3),
    }
}

async fn run() -> Result<(), IngressExitError> {
    let mut input = Vec::new();
    io::stdin()
        .take((MAX_PROTOCOL_BYTES + 1) as u64)
        .read_to_end(&mut input)
        .map_err(SpoolError::ReadFailed)?;
    let envelope = IngressEvent::parse(&input)?;
    let root = default_spool_dir().ok_or(SpoolError::InvalidPath)?;
    let spool = Spool::open(root, SpoolLimits::default())?;
    submit_event(&envelope, &spool).await?;
    Ok(())
}

enum IngressExitError {
    Protocol,
    Spool,
}

impl From<IngressError> for IngressExitError {
    fn from(_value: IngressError) -> Self {
        Self::Protocol
    }
}

impl From<SpoolError> for IngressExitError {
    fn from(_value: SpoolError) -> Self {
        Self::Spool
    }
}

#[cfg(windows)]
async fn submit_event(
    envelope: &agentnotify_agent_sdk::AgentEventEnvelope,
    spool: &Spool,
) -> Result<SubmitResult, SpoolError> {
    let name = pipe_name().ok();
    submit_with_fallback(envelope, name.as_deref(), spool, DEFAULT_CONNECT_TIMEOUT).await
}

#[cfg(not(windows))]
async fn submit_event(
    envelope: &agentnotify_agent_sdk::AgentEventEnvelope,
    spool: &Spool,
) -> Result<(), SpoolError> {
    spool.write_event(envelope)?;
    Ok(())
}

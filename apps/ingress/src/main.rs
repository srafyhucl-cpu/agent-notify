#![cfg_attr(windows, windows_subsystem = "windows")]

use std::{
    io::{self, Read},
    process::ExitCode,
};

use agentnotify_ingress::{
    IngressError, IngressEvent, SpoolError, protocol::MAX_PROTOCOL_BYTES, write_default_spool,
};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(IngressExitError::Protocol) => ExitCode::from(2),
        Err(IngressExitError::Spool) => ExitCode::from(3),
    }
}

fn run() -> Result<(), IngressExitError> {
    let mut input = Vec::new();
    io::stdin()
        .take((MAX_PROTOCOL_BYTES + 1) as u64)
        .read_to_end(&mut input)
        .map_err(SpoolError::ReadFailed)?;
    let envelope = IngressEvent::parse(&input)?;
    write_default_spool(&envelope)?;
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

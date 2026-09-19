pub mod protocol;
pub mod spool;
#[cfg(windows)]
pub mod windows_pipe;

pub use protocol::{IngressError, IngressEvent};
pub use spool::{
    Spool, SpoolEntry, SpoolError, SpoolLimits, default_spool_dir, write_default_spool,
};

#[cfg(windows)]
pub use windows_pipe::{
    DEFAULT_CONNECT_TIMEOUT, HandlerError, IngressHandler, PIPE_NAME_PREFIX, PipeError,
    SubmitResult, connect_and_submit, current_user_sddl, pipe_name, serve, serve_on,
    submit_with_fallback,
};

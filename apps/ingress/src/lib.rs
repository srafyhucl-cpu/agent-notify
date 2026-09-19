pub mod protocol;
pub mod spool;

pub use protocol::{IngressError, IngressEvent};
pub use spool::{
    Spool, SpoolEntry, SpoolError, SpoolLimits, default_spool_dir, write_default_spool,
};

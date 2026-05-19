//! deltagen library root
pub mod apply;
pub mod delta;
pub mod snapshot;
pub mod util;

pub use apply::apply_patch;
pub use delta::{compute_delta, invert_patch, Operation};
pub use snapshot::Snapshot;

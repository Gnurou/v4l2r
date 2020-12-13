use super::BufferHandles;
use crate::ioctl;

use std::sync::{Arc, Mutex};

/// Represents the current state of an allocated buffer.
pub(super) enum BufferState<P: BufferHandles> {
    /// The buffer can be obtained via `get_buffer()` and be queued.
    Free,
    /// The buffer has been requested via `get_buffer()` but is not queued yet.
    PreQueue,
    /// The buffer is queued and waiting to be dequeued.
    Queued(P),
    /// The buffer has been dequeued and the client is still using it. The buffer
    /// will go back to the `Free` state once the reference is dropped.
    Dequeued,
}

pub(super) struct BufferInfo<P: BufferHandles> {
    pub(super) state: Arc<Mutex<BufferState<P>>>,
    pub(super) features: Arc<ioctl::QueryBuffer>,
}

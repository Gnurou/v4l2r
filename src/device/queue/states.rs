use super::PlaneHandles;
use crate::ioctl;
use crate::memory::Memory;

use std::sync::{Arc, Mutex};

/// Represents the current state of an allocated buffer.
pub(super) enum BufferState<M: Memory> {
    /// The buffer can be obtained via `get_buffer()` and be queued.
    Free,
    /// The buffer has been requested via `get_buffer()` but is not queued yet.
    PreQueue,
    /// The buffer is queued and waiting to be dequeued.
    Queued(PlaneHandles<M>),
    /// The buffer has been dequeued and the client is still using it. The buffer
    /// will go back to the `Free` state once the reference is dropped.
    Dequeued,
}

pub(super) struct BufferInfo<M: Memory> {
    pub(super) state: Arc<Mutex<BufferState<M>>>,
    pub(super) features: Arc<ioctl::QueryBuffer>,
}

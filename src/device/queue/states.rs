use super::PlaneHandles;
use crate::ioctl;
use crate::memory::Memory;
use std::collections::VecDeque;

use std::sync::{Arc, Mutex};

pub(super) trait BufferAllocator: Send + Sync {
    fn get_free_buffer(&self) -> Option<usize>;
    fn take_buffer(&self, index: usize);
    fn return_buffer(&self, index: usize);
}

pub(super) struct FifoBufferAllocator {
    queue: Mutex<VecDeque<usize>>,
}

impl FifoBufferAllocator {
    pub(super) fn new(nb_buffers: usize) -> Self {
        FifoBufferAllocator {
            queue: Mutex::new((0..nb_buffers).collect()),
        }
    }
}

impl BufferAllocator for FifoBufferAllocator {
    fn get_free_buffer(&self) -> Option<usize> {
        self.queue.lock().unwrap().front().copied()
    }

    fn take_buffer(&self, index: usize) {
        self.queue.lock().unwrap().retain(|i| *i != index);
    }

    fn return_buffer(&self, index: usize) {
        self.queue.lock().unwrap().push_back(index);
    }
}

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

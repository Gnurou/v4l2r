use super::PlaneHandles;
use crate::ioctl;
use crate::memory::Memory;
use std::collections::VecDeque;

use std::sync::{Arc, Mutex};

/// Trait for the different states a queue can be in. This allows us to limit
/// the available queue methods to the one that make sense at a given point of
/// the queue's lifecycle.
pub trait QueueState {}

/// Initial state of the queue when created. Streaming and queuing are not
/// supported since buffers have not been allocated yet.
/// Allocating buffers makes the queue switch to the `BuffersAllocated` state.
pub struct QueueInit;
impl QueueState for QueueInit {}

pub(super) trait BufferAllocator {
    fn get_free_buffer(&mut self) -> Option<usize>;
    fn take_buffer(&mut self, index: usize);
    fn return_buffer(&mut self, index: usize);
}

pub(super) struct FifoBufferAllocator {
    queue: VecDeque<usize>,
}

impl FifoBufferAllocator {
    fn new(nb_buffers: usize) -> Self {
        FifoBufferAllocator {
            queue: (0..nb_buffers).collect(),
        }
    }
}

impl BufferAllocator for FifoBufferAllocator {
    fn get_free_buffer(&mut self) -> Option<usize> {
        self.queue.pop_front()
    }

    fn take_buffer(&mut self, index: usize) {
        self.queue.retain(|i| *i != index);
    }

    fn return_buffer(&mut self, index: usize) {
        self.queue.push_back(index);
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

pub(super) struct BuffersManager<M: Memory> {
    pub(super) allocator: FifoBufferAllocator,
    pub(super) buffers_state: Vec<BufferState<M>>,
    pub(super) num_queued_buffers: usize,
}


impl<M: Memory> BuffersManager<M> {
    pub(super) fn new(num_buffers: usize) -> Self {
        BuffersManager {
            allocator: FifoBufferAllocator::new(num_buffers),
            buffers_state: std::iter::repeat_with(|| BufferState::Free)
                .take(num_buffers)
                .collect(),
            num_queued_buffers: 0,
        }
    }
}

/// Allocated state for a queue. A queue with its buffers allocated can be
/// streamed on and off, and buffers can be queued and dequeued.
pub struct BuffersAllocated<M: Memory> {
    pub(super) num_buffers: usize,
    pub(super) buffers_state: Arc<Mutex<BuffersManager<M>>>,
    pub(super) buffer_features: ioctl::QueryBuffer,
}
impl<M: Memory> QueueState for BuffersAllocated<M> {}

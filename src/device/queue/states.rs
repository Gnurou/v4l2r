use crate::memory::Memory;
use crate::ioctl;
use super::BufferState;

use std::cell::RefCell;
use std::rc::Rc;

/// Trait for the different states a queue can be in. This allows us to limit
/// the available queue methods to the one that make sense at a given point of
/// the queue's lifecycle.
pub trait QueueState {}

/// Initial state of the queue when created. Streaming and queuing are not
/// supported since buffers have not been allocated yet.
/// Allocating buffers makes the queue switch to the `BuffersAllocated` state.
pub struct QueueInit;
impl QueueState for QueueInit {}

/// Allocated state for a queue. A queue with its buffers allocated can be
/// streamed on and off, and buffers can be queued and dequeued.
pub struct BuffersAllocated<M: Memory> {
    pub(super) buffers_state: Vec<Rc<RefCell<BufferState<M>>>>,
    pub(super) buffer_features: ioctl::QueryBuffer,
}
impl<M: Memory> QueueState for BuffersAllocated<M> {}

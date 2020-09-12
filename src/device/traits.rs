use ioctl::DQBufResult;

use super::queue::{direction::Direction, qbuf::get_indexed::GetBufferByIndex, Queue, QueueInit};
use crate::ioctl;
use std::fmt::Debug;

/// Trait for trying to dequeue a readable buffer from a queue.
///
/// DQB is the type of dequeued buffers returned by the implementor.
pub trait TryDequeue {
    type Dequeued: Debug + Send;

    /// Try to dequeue and return the next processed buffer.
    ///
    /// The V4L2 buffer will not be reused until the returned value is dropped.
    /// It can be moved into a `Rc` or `Arc` and passed across threads.
    ///
    /// The data in the `DQBuffer` is read-only.
    fn try_dequeue(&self) -> DQBufResult<Self::Dequeued>;
}

/// Trait for streaming a queue on and off.
///
/// The `CB` generic type is the type
/// for returned cancelled buffers, i.e. buffers that were queued prior to the
/// call to `stream_off()` but were not yet dequeued.
pub trait Stream {
    type Canceled;

    /// Start streaming. Buffers queued prior to calling this method will start
    /// being processed.
    fn stream_on(&self) -> Result<(), ioctl::StreamOnError>;

    /// Stop streaming.
    ///
    /// If successful, then all the buffers that are queued but have not been
    /// dequeued yet return to the `Free` sta, and be returned as `Canceled`.
    fn stream_off(&self) -> Result<Vec<Self::Canceled>, ioctl::StreamOffError>;
}

/// Trait for a configured queue, i.e. a queue on which we can queue and dequeue
/// buffers.
pub trait AllocatedQueue<'a, D: Direction>: GetBufferByIndex<'a> + TryDequeue + Stream {
    /// Returns the total number of buffers allocated for this queue.
    fn num_buffers(&self) -> usize;

    /// Returns the number of buffers currently queued (i.e. being processed
    /// or awaiting to be dequeued).
    fn num_queued_buffers(&self) -> usize;

    /// Release all the allocated buffers and returns the queue to the `Init` state.
    fn free_buffers(self) -> Result<Queue<D, QueueInit>, ioctl::ReqbufsError>;
}

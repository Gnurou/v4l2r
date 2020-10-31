use ioctl::DQBufResult;

use super::{
    direction::Direction,
    dqbuf::DQBuffer,
    qbuf::{
        get_indexed::{GetBufferByIndex, TryGetBufferError},
        QBuffer,
    },
    BuffersAllocated, CanceledBuffer, Queue,
};
use crate::{
    device::AllocatedQueue,
    device::{Stream, TryDequeue},
    ioctl,
    memory::{Memory, UserPtr, MMAP},
};
use std::fmt::Debug;

// TODO use a macro to do this. That way we don't need to rely on external crates as we
// can to the From<> implementations ourselves.

pub enum DualQBuffer<'a, D: Direction> {
    MMAP(QBuffer<'a, D, MMAP>),
    User(QBuffer<'a, D, UserPtr<Vec<u8>>>),
}

impl<'a, D: Direction> From<QBuffer<'a, D, MMAP>> for DualQBuffer<'a, D> {
    fn from(qb: QBuffer<'a, D, MMAP>) -> Self {
        DualQBuffer::MMAP(qb)
    }
}

impl<'a, D: Direction> From<QBuffer<'a, D, UserPtr<Vec<u8>>>> for DualQBuffer<'a, D> {
    fn from(qb: QBuffer<'a, D, UserPtr<Vec<u8>>>) -> Self {
        DualQBuffer::User(qb)
    }
}

#[derive(Debug)]
pub enum DualDQBuffer<D: Direction> {
    MMAP(DQBuffer<D, MMAP>),
    User(DQBuffer<D, UserPtr<Vec<u8>>>),
}

impl<D: Direction> DualDQBuffer<D> {
    /// Accessor to the V4L2 dequeue data without requiring the caller to
    /// precisely match the type.
    pub fn data(&self) -> &ioctl::DQBuffer {
        match self {
            DualDQBuffer::MMAP(m) => &m.data,
            DualDQBuffer::User(u) => &u.data,
        }
    }
}

impl<D: Direction> From<DQBuffer<D, MMAP>> for DualDQBuffer<D> {
    fn from(dqb: DQBuffer<D, MMAP>) -> Self {
        DualDQBuffer::MMAP(dqb)
    }
}

impl<D: Direction> From<DQBuffer<D, UserPtr<Vec<u8>>>> for DualDQBuffer<D> {
    fn from(dqb: DQBuffer<D, UserPtr<Vec<u8>>>) -> Self {
        DualDQBuffer::User(dqb)
    }
}

pub enum DualCanceledBuffer {
    MMAP(CanceledBuffer<MMAP>),
    User(CanceledBuffer<UserPtr<Vec<u8>>>),
}

impl From<CanceledBuffer<MMAP>> for DualCanceledBuffer {
    fn from(cb: CanceledBuffer<MMAP>) -> Self {
        DualCanceledBuffer::MMAP(cb)
    }
}

impl From<CanceledBuffer<UserPtr<Vec<u8>>>> for DualCanceledBuffer {
    fn from(cb: CanceledBuffer<UserPtr<Vec<u8>>>) -> Self {
        DualCanceledBuffer::User(cb)
    }
}

pub enum DualAllocatedQueue<D: Direction> {
    MMAP(Queue<D, BuffersAllocated<MMAP>>),
    User(Queue<D, BuffersAllocated<UserPtr<Vec<u8>>>>),
}

impl<D: Direction> From<Queue<D, BuffersAllocated<MMAP>>> for DualAllocatedQueue<D> {
    fn from(q: Queue<D, BuffersAllocated<MMAP>>) -> Self {
        DualAllocatedQueue::MMAP(q)
    }
}

impl<D: Direction> From<Queue<D, BuffersAllocated<UserPtr<Vec<u8>>>>> for DualAllocatedQueue<D> {
    fn from(q: Queue<D, BuffersAllocated<UserPtr<Vec<u8>>>>) -> Self {
        DualAllocatedQueue::User(q)
    }
}

impl<D: Direction> Stream for DualAllocatedQueue<D> {
    type Canceled = DualCanceledBuffer;

    fn stream_on(&self) -> Result<(), ioctl::StreamOnError> {
        match self {
            DualAllocatedQueue::MMAP(m) => m.stream_on(),
            DualAllocatedQueue::User(u) => u.stream_on(),
        }
    }

    fn stream_off(&self) -> Result<Vec<DualCanceledBuffer>, ioctl::StreamOffError> {
        match self {
            // TODO we should include the Vec inside the enum, probably?
            DualAllocatedQueue::MMAP(m) => m
                .stream_off()
                .map(|v| v.into_iter().map(DualCanceledBuffer::from).collect()),
            DualAllocatedQueue::User(u) => u
                .stream_off()
                .map(|v| v.into_iter().map(DualCanceledBuffer::from).collect()),
        }
    }
}

impl<'a, D: Direction> GetBufferByIndex<'a> for DualAllocatedQueue<D> {
    type Queueable = DualQBuffer<'a, D>;

    fn try_get_buffer(&'a self, index: usize) -> Result<Self::Queueable, TryGetBufferError> {
        match self {
            DualAllocatedQueue::MMAP(m) => m.try_get_buffer(index).map(DualQBuffer::from),
            DualAllocatedQueue::User(u) => u.try_get_buffer(index).map(DualQBuffer::from),
        }
    }
}

fn map_dqbuferror_to_dual<D: Direction, M: Memory>(
    e: ioctl::DQBufError<DQBuffer<D, M>>,
) -> ioctl::DQBufError<DualDQBuffer<D>>
where
    DQBuffer<D, M>: Into<DualDQBuffer<D>>,
{
    match e {
        ioctl::DQBufError::EOS => ioctl::DQBufError::EOS,
        ioctl::DQBufError::NotReady => ioctl::DQBufError::NotReady,
        ioctl::DQBufError::CorruptedBuffer(b) => ioctl::DQBufError::CorruptedBuffer(b.into()),
        ioctl::DQBufError::IoctlError(e) => ioctl::DQBufError::IoctlError(e),
    }
}

impl<D: Direction> TryDequeue for DualAllocatedQueue<D> {
    type Dequeued = DualDQBuffer<D>;

    fn try_dequeue(&self) -> DQBufResult<Self::Dequeued> {
        match self {
            DualAllocatedQueue::MMAP(m) => m
                .try_dequeue()
                .map(DualDQBuffer::from)
                .map_err(map_dqbuferror_to_dual),
            DualAllocatedQueue::User(u) => u
                .try_dequeue()
                .map(DualDQBuffer::from)
                .map_err(map_dqbuferror_to_dual),
        }
    }
}

impl<'a, D: Direction> AllocatedQueue<'a, D> for DualAllocatedQueue<D> {
    fn num_buffers(&self) -> usize {
        match self {
            DualAllocatedQueue::MMAP(m) => m.num_buffers(),
            DualAllocatedQueue::User(u) => u.num_buffers(),
        }
    }

    fn num_queued_buffers(&self) -> usize {
        match self {
            DualAllocatedQueue::MMAP(m) => m.num_queued_buffers(),
            DualAllocatedQueue::User(u) => u.num_queued_buffers(),
        }
    }

    fn free_buffers(self) -> Result<Queue<D, super::QueueInit>, ioctl::ReqbufsError> {
        match self {
            DualAllocatedQueue::MMAP(m) => m.free_buffers(),
            DualAllocatedQueue::User(u) => u.free_buffers(),
        }
    }
}

use crate::{
    device::queue::{
        direction::{Capture, Direction, Output},
        qbuf::{QBuffer, QueueResult},
        BuffersAllocated, CaptureQueueable, OutputQueueable, Queue,
    },
    memory::DmaBufHandle,
};
use crate::{
    memory::MmapHandle,
    memory::{BufferHandles, MemoryType, UserPtrHandle},
};
use std::{fmt::Debug, fs::File, ops::Deref};

/// Supported memory types for `GenericBufferHandles`.
/// TODO: This should be renamed to "DynamicBufferHandles", and be constructed
/// on-the-fly using a macro.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GenericSupportedMemoryType {
    Mmap,
    UserPtr,
    DmaBuf,
}

impl From<GenericSupportedMemoryType> for MemoryType {
    fn from(mem_type: GenericSupportedMemoryType) -> Self {
        match mem_type {
            GenericSupportedMemoryType::Mmap => MemoryType::Mmap,
            GenericSupportedMemoryType::UserPtr => MemoryType::UserPtr,
            GenericSupportedMemoryType::DmaBuf => MemoryType::DmaBuf,
        }
    }
}

/// Buffer handle capable of holding either MMAP or UserPtr handles. Useful
/// for cases when we want to decide the memory type of a queue at runtime.
#[derive(Debug)]
pub enum GenericBufferHandles {
    Mmap(Vec<MmapHandle>),
    User(Vec<UserPtrHandle<Vec<u8>>>),
    DmaBuf(Vec<DmaBufHandle<File>>),
}

impl From<Vec<MmapHandle>> for GenericBufferHandles {
    fn from(m: Vec<MmapHandle>) -> Self {
        Self::Mmap(m)
    }
}

impl From<Vec<UserPtrHandle<Vec<u8>>>> for GenericBufferHandles {
    fn from(u: Vec<UserPtrHandle<Vec<u8>>>) -> Self {
        Self::User(u)
    }
}

impl From<Vec<DmaBufHandle<File>>> for GenericBufferHandles {
    fn from(d: Vec<DmaBufHandle<File>>) -> Self {
        Self::DmaBuf(d)
    }
}

impl BufferHandles for GenericBufferHandles {
    type SupportedMemoryType = GenericSupportedMemoryType;

    fn len(&self) -> usize {
        match self {
            GenericBufferHandles::Mmap(m) => m.len(),
            GenericBufferHandles::User(u) => u.len(),
            GenericBufferHandles::DmaBuf(d) => d.len(),
        }
    }

    fn fill_v4l2_plane(&self, index: usize, plane: &mut crate::bindings::v4l2_plane) {
        match self {
            GenericBufferHandles::Mmap(m) => m.fill_v4l2_plane(index, plane),
            GenericBufferHandles::User(u) => u.fill_v4l2_plane(index, plane),
            GenericBufferHandles::DmaBuf(d) => d.fill_v4l2_plane(index, plane),
        }
    }
}

/// A QBuffer that holds either MMAP or UserPtr handles, depending on which
/// memory type has been selected for the queue at runtime.
pub enum GenericQBuffer<
    D: Direction,
    Q: Deref<Target = Queue<D, BuffersAllocated<GenericBufferHandles>>>,
> {
    Mmap(QBuffer<D, Vec<MmapHandle>, GenericBufferHandles, Q>),
    User(QBuffer<D, Vec<UserPtrHandle<Vec<u8>>>, GenericBufferHandles, Q>),
    DmaBuf(QBuffer<D, Vec<DmaBufHandle<File>>, GenericBufferHandles, Q>),
}

impl<D, Q> From<QBuffer<D, Vec<MmapHandle>, GenericBufferHandles, Q>> for GenericQBuffer<D, Q>
where
    D: Direction,
    Q: Deref<Target = Queue<D, BuffersAllocated<GenericBufferHandles>>>,
{
    fn from(qb: QBuffer<D, Vec<MmapHandle>, GenericBufferHandles, Q>) -> Self {
        GenericQBuffer::Mmap(qb)
    }
}

impl<D, Q> From<QBuffer<D, Vec<UserPtrHandle<Vec<u8>>>, GenericBufferHandles, Q>>
    for GenericQBuffer<D, Q>
where
    D: Direction,
    Q: Deref<Target = Queue<D, BuffersAllocated<GenericBufferHandles>>>,
{
    fn from(qb: QBuffer<D, Vec<UserPtrHandle<Vec<u8>>>, GenericBufferHandles, Q>) -> Self {
        GenericQBuffer::User(qb)
    }
}

impl<D, Q> From<QBuffer<D, Vec<DmaBufHandle<File>>, GenericBufferHandles, Q>>
    for GenericQBuffer<D, Q>
where
    D: Direction,
    Q: Deref<Target = Queue<D, BuffersAllocated<GenericBufferHandles>>>,
{
    fn from(qb: QBuffer<D, Vec<DmaBufHandle<File>>, GenericBufferHandles, Q>) -> Self {
        GenericQBuffer::DmaBuf(qb)
    }
}

/// Any CAPTURE GenericQBuffer implements CaptureQueueable.
impl<Q> CaptureQueueable<GenericBufferHandles> for GenericQBuffer<Capture, Q>
where
    Q: Deref<Target = Queue<Capture, BuffersAllocated<GenericBufferHandles>>>,
{
    fn queue_with_handles(
        self,
        handles: GenericBufferHandles,
    ) -> QueueResult<(), GenericBufferHandles> {
        match self {
            GenericQBuffer::Mmap(m) => m.queue_with_handles(handles),
            GenericQBuffer::User(u) => u.queue_with_handles(handles),
            GenericQBuffer::DmaBuf(d) => d.queue_with_handles(handles),
        }
    }
}

/// Any OUTPUT GenericQBuffer implements OutputQueueable.
impl<Q> OutputQueueable<GenericBufferHandles> for GenericQBuffer<Output, Q>
where
    Q: Deref<Target = Queue<Output, BuffersAllocated<GenericBufferHandles>>>,
{
    fn queue_with_handles(
        self,
        handles: GenericBufferHandles,
        bytes_used: &[usize],
    ) -> QueueResult<(), GenericBufferHandles> {
        match self {
            GenericQBuffer::Mmap(m) => m.queue_with_handles(handles, bytes_used),
            GenericQBuffer::User(u) => u.queue_with_handles(handles, bytes_used),
            GenericQBuffer::DmaBuf(d) => d.queue_with_handles(handles, bytes_used),
        }
    }
}

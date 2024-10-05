use crate::{
    device::queue::{
        direction::{Capture, Direction, Output},
        qbuf::{CaptureQueueable, OutputQueueable, QBuffer, QueueResult},
    },
    memory::DmaBufHandle,
};
use crate::{
    memory::MmapHandle,
    memory::{BufferHandles, MemoryType, UserPtrHandle},
};
use std::{fmt::Debug, fs::File};

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
pub enum GenericQBuffer<'a, D: Direction> {
    Mmap(QBuffer<'a, D, Vec<MmapHandle>, GenericBufferHandles>),
    User(QBuffer<'a, D, Vec<UserPtrHandle<Vec<u8>>>, GenericBufferHandles>),
    DmaBuf(QBuffer<'a, D, Vec<DmaBufHandle<File>>, GenericBufferHandles>),
}

impl<'a, D: Direction> From<QBuffer<'a, D, Vec<MmapHandle>, GenericBufferHandles>>
    for GenericQBuffer<'a, D>
{
    fn from(qb: QBuffer<'a, D, Vec<MmapHandle>, GenericBufferHandles>) -> Self {
        GenericQBuffer::Mmap(qb)
    }
}

impl<'a, D: Direction> From<QBuffer<'a, D, Vec<UserPtrHandle<Vec<u8>>>, GenericBufferHandles>>
    for GenericQBuffer<'a, D>
{
    fn from(qb: QBuffer<'a, D, Vec<UserPtrHandle<Vec<u8>>>, GenericBufferHandles>) -> Self {
        GenericQBuffer::User(qb)
    }
}

impl<'a, D: Direction> From<QBuffer<'a, D, Vec<DmaBufHandle<File>>, GenericBufferHandles>>
    for GenericQBuffer<'a, D>
{
    fn from(qb: QBuffer<'a, D, Vec<DmaBufHandle<File>>, GenericBufferHandles>) -> Self {
        GenericQBuffer::DmaBuf(qb)
    }
}

/// Any CAPTURE GenericQBuffer implements CaptureQueueable.
impl CaptureQueueable<GenericBufferHandles> for GenericQBuffer<'_, Capture> {
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
impl OutputQueueable<GenericBufferHandles> for GenericQBuffer<'_, Output> {
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

use crate::{
    device::queue::{
        direction::{Capture, Direction, Output},
        qbuf::{CaptureQueueable, OutputQueueable, QBuffer, QueueResult},
    },
    memory::DMABufHandle,
};
use crate::{
    memory::MMAPHandle,
    memory::{BufferHandles, MemoryType, UserPtrHandle},
};
use std::{fmt::Debug, fs::File};

/// Supported memory types for `GenericBufferHandles`.
/// TODO: This should be renamed to "DynamicBufferHandles", and be constructed
/// on-the-fly using a macro.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GenericSupportedMemoryType {
    MMAP,
    UserPtr,
    DMABuf,
}

impl Into<MemoryType> for GenericSupportedMemoryType {
    fn into(self) -> MemoryType {
        match self {
            GenericSupportedMemoryType::MMAP => MemoryType::MMAP,
            GenericSupportedMemoryType::UserPtr => MemoryType::UserPtr,
            GenericSupportedMemoryType::DMABuf => MemoryType::DMABuf,
        }
    }
}

/// Buffer handle capable of holding either MMAP or UserPtr handles. Useful
/// for cases when we want to decide the memory type of a queue at runtime.
#[derive(Debug)]
pub enum GenericBufferHandles {
    MMAP(Vec<MMAPHandle>),
    User(Vec<UserPtrHandle<Vec<u8>>>),
    DMABuf(Vec<DMABufHandle<File>>),
}

impl From<Vec<MMAPHandle>> for GenericBufferHandles {
    fn from(m: Vec<MMAPHandle>) -> Self {
        Self::MMAP(m)
    }
}

impl From<Vec<UserPtrHandle<Vec<u8>>>> for GenericBufferHandles {
    fn from(u: Vec<UserPtrHandle<Vec<u8>>>) -> Self {
        Self::User(u)
    }
}

impl From<Vec<DMABufHandle<File>>> for GenericBufferHandles {
    fn from(d: Vec<DMABufHandle<File>>) -> Self {
        Self::DMABuf(d)
    }
}

impl BufferHandles for GenericBufferHandles {
    type SupportedMemoryType = GenericSupportedMemoryType;

    fn len(&self) -> usize {
        match self {
            GenericBufferHandles::MMAP(m) => m.len(),
            GenericBufferHandles::User(u) => u.len(),
            GenericBufferHandles::DMABuf(d) => d.len(),
        }
    }

    fn fill_v4l2_plane(&self, index: usize, plane: &mut crate::bindings::v4l2_plane) {
        match self {
            GenericBufferHandles::MMAP(m) => m.fill_v4l2_plane(index, plane),
            GenericBufferHandles::User(u) => u.fill_v4l2_plane(index, plane),
            GenericBufferHandles::DMABuf(d) => d.fill_v4l2_plane(index, plane),
        }
    }
}

/// A QBuffer that holds either MMAP or UserPtr handles, depending on which
/// memory type has been selected for the queue at runtime.
pub enum GenericQBuffer<'a, D: Direction> {
    MMAP(QBuffer<'a, D, Vec<MMAPHandle>, GenericBufferHandles>),
    User(QBuffer<'a, D, Vec<UserPtrHandle<Vec<u8>>>, GenericBufferHandles>),
    DMABuf(QBuffer<'a, D, Vec<DMABufHandle<File>>, GenericBufferHandles>),
}

impl<'a, D: Direction> From<QBuffer<'a, D, Vec<MMAPHandle>, GenericBufferHandles>>
    for GenericQBuffer<'a, D>
{
    fn from(qb: QBuffer<'a, D, Vec<MMAPHandle>, GenericBufferHandles>) -> Self {
        GenericQBuffer::MMAP(qb)
    }
}

impl<'a, D: Direction> From<QBuffer<'a, D, Vec<UserPtrHandle<Vec<u8>>>, GenericBufferHandles>>
    for GenericQBuffer<'a, D>
{
    fn from(qb: QBuffer<'a, D, Vec<UserPtrHandle<Vec<u8>>>, GenericBufferHandles>) -> Self {
        GenericQBuffer::User(qb)
    }
}

impl<'a, D: Direction> From<QBuffer<'a, D, Vec<DMABufHandle<File>>, GenericBufferHandles>>
    for GenericQBuffer<'a, D>
{
    fn from(qb: QBuffer<'a, D, Vec<DMABufHandle<File>>, GenericBufferHandles>) -> Self {
        GenericQBuffer::DMABuf(qb)
    }
}

/// Any CAPTURE GenericQBuffer implements CaptureQueueable.
impl CaptureQueueable<GenericBufferHandles> for GenericQBuffer<'_, Capture> {
    fn queue_with_handles(
        self,
        handles: GenericBufferHandles,
    ) -> QueueResult<(), GenericBufferHandles> {
        match self {
            GenericQBuffer::MMAP(m) => m.queue_with_handles(handles),
            GenericQBuffer::User(u) => u.queue_with_handles(handles),
            GenericQBuffer::DMABuf(d) => d.queue_with_handles(handles),
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
            GenericQBuffer::MMAP(m) => m.queue_with_handles(handles, bytes_used),
            GenericQBuffer::User(u) => u.queue_with_handles(handles, bytes_used),
            GenericQBuffer::DMABuf(d) => d.queue_with_handles(handles, bytes_used),
        }
    }
}

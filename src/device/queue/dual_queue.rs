use crate::device::queue::{
    direction::{Capture, Direction, Output},
    qbuf::{CaptureQueueable, OutputQueueable, QBuffer, QueueResult},
};
use crate::{
    memory::MMAPHandle,
    memory::{BufferHandles, MemoryType, UserPtrHandle},
};
use std::fmt::Debug;

/// Supported memory types for `DualBufferHandles`.
/// TODO: This should be renamed to "DynamicBufferHandles", and be constructed
/// on-the-fly using a macro.
#[derive(Debug, Clone, Copy)]
pub enum DualSupportedMemoryType {
    MMAP,
    UserPtr,
}

impl Into<MemoryType> for DualSupportedMemoryType {
    fn into(self) -> MemoryType {
        match self {
            DualSupportedMemoryType::MMAP => MemoryType::MMAP,
            DualSupportedMemoryType::UserPtr => MemoryType::UserPtr,
        }
    }
}

/// Buffer handle capable of holding either MMAP or UserPtr handles. Useful
/// for cases when we want to decide the memory type of a queue at runtime.
#[derive(Debug)]
pub enum DualBufferHandles {
    MMAP(Vec<MMAPHandle>),
    User(Vec<UserPtrHandle<Vec<u8>>>),
}

impl From<Vec<MMAPHandle>> for DualBufferHandles {
    fn from(m: Vec<MMAPHandle>) -> Self {
        Self::MMAP(m)
    }
}

impl From<Vec<UserPtrHandle<Vec<u8>>>> for DualBufferHandles {
    fn from(u: Vec<UserPtrHandle<Vec<u8>>>) -> Self {
        Self::User(u)
    }
}

impl BufferHandles for DualBufferHandles {
    type SupportedMemoryType = DualSupportedMemoryType;

    fn len(&self) -> usize {
        match self {
            DualBufferHandles::MMAP(m) => m.len(),
            DualBufferHandles::User(u) => u.len(),
        }
    }

    fn fill_v4l2_plane(&self, index: usize, plane: &mut crate::bindings::v4l2_plane) {
        match self {
            DualBufferHandles::MMAP(m) => m.fill_v4l2_plane(index, plane),
            DualBufferHandles::User(u) => u.fill_v4l2_plane(index, plane),
        }
    }
}

/// A QBuffer that holds either MMAP or UserPtr handles, depending on which
/// memory type has been selected for the queue at runtime.
pub enum DualQBuffer<'a, D: Direction> {
    MMAP(QBuffer<'a, D, Vec<MMAPHandle>, DualBufferHandles>),
    User(QBuffer<'a, D, Vec<UserPtrHandle<Vec<u8>>>, DualBufferHandles>),
}

impl<'a, D: Direction> From<QBuffer<'a, D, Vec<MMAPHandle>, DualBufferHandles>>
    for DualQBuffer<'a, D>
{
    fn from(qb: QBuffer<'a, D, Vec<MMAPHandle>, DualBufferHandles>) -> Self {
        DualQBuffer::MMAP(qb)
    }
}

impl<'a, D: Direction> From<QBuffer<'a, D, Vec<UserPtrHandle<Vec<u8>>>, DualBufferHandles>>
    for DualQBuffer<'a, D>
{
    fn from(qb: QBuffer<'a, D, Vec<UserPtrHandle<Vec<u8>>>, DualBufferHandles>) -> Self {
        DualQBuffer::User(qb)
    }
}

/// Any CAPTURE DualQBuffer implements CaptureQueueable.
impl CaptureQueueable<DualBufferHandles> for DualQBuffer<'_, Capture> {
    fn queue_with_handles(self, handles: DualBufferHandles) -> QueueResult<(), DualBufferHandles> {
        match self {
            DualQBuffer::MMAP(m) => m.queue_with_handles(handles),
            DualQBuffer::User(u) => u.queue_with_handles(handles),
        }
    }
}

/// Any OUTPUT DualQBuffer implements OutputQueueable.
impl OutputQueueable<DualBufferHandles> for DualQBuffer<'_, Output> {
    fn queue_with_handles(
        self,
        handles: DualBufferHandles,
        bytes_used: &[usize],
    ) -> QueueResult<(), DualBufferHandles> {
        match self {
            DualQBuffer::MMAP(m) => m.queue_with_handles(handles, bytes_used),
            DualQBuffer::User(u) => u.queue_with_handles(handles, bytes_used),
        }
    }
}

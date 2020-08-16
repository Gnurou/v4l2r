//! Operations specific to MMAP-type buffers.
use super::*;
use crate::{bindings, ioctl};
use std::{
    fmt::{self, Debug},
    sync::Weak,
};

/// Handle for a MMAP buffer. These buffers are backed by V4L2 itself, and
/// thus we don't need to attach any extra handle information to them. We
/// still need to pass an mmap offset to user-space so it can read/write the
/// data.
pub struct MMAPHandle(u32);

impl MMAPHandle {
    pub fn new(index: u32) -> MMAPHandle {
        MMAPHandle(index)
    }
}

impl Debug for MMAPHandle {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_tuple("MMAPHandle").field(&self.0).finish()
    }
}

impl PlaneHandle for MMAPHandle {
    const MEMORY_TYPE: MemoryType = MemoryType::MMAP;

    // There is no information to fill with MMAP buffers ; the index is enough.
    fn fill_v4l2_buffer(_plane: &bindings::v4l2_plane, _buffer: &mut bindings::v4l2_buffer) {}
    fn fill_v4l2_plane(&self, _plane: &mut bindings::v4l2_plane) {}
}

pub struct MMAPMapper {
    device: Weak<Device>,
    mem_offset: u32,
    length: u32,
}

impl PlaneMapper for MMAPMapper {
    fn new(device: &Arc<Device>, mem_offset: u32, length: u32) -> Self {
        MMAPMapper {
            device: Arc::downgrade(&device),
            mem_offset,
            length,
        }
    }

    fn map(&self) -> Option<PlaneMapping> {
        // TODO create appropriate error. And return Result<>?
        let device = self.device.upgrade()?;
        Some(ioctl::mmap(&*device, self.mem_offset, self.length).ok()?)
    }
}

pub struct MMAP;

/// MMAP buffers support for queues. These buffers are the easiest to support
/// since V4L2 is the owner of the backing memory. Therefore we just need to
/// make sure that userspace does not have access to any mapping while the
/// buffer is being processed by the kernel.
impl Memory for MMAP {
    type HandleType = MMAPHandle;
    type Type = Fixed;
    type MapperType = MMAPMapper;
}

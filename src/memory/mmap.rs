//! Operations specific to MMAP-type buffers.
use super::*;
use crate::bindings;
use std::fmt::{self, Debug};

/// Handle for a MMAP buffer. These buffers are backed by V4L2 itself, and
/// thus we don't need to attach any extra handle information to them. We
/// still need to pass an mmap offset to user-space so it can read/write the
/// data.
#[derive(Default)]
pub struct MMAPHandle(u32);

impl Debug for MMAPHandle {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_tuple("MMAPHandle")
            .field(&format!("{:#010x}", self.0))
            .finish()
    }
}

impl PlaneHandle for MMAPHandle {
    const MEMORY_TYPE: MemoryType = MemoryType::MMAP;

    // There is no information to fill with MMAP buffers ; the index is enough.
    fn fill_v4l2_buffer(&self, _buffer: &mut bindings::v4l2_buffer) {}
    fn fill_v4l2_plane(&self, _plane: &mut bindings::v4l2_plane) {}
}

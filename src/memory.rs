//! Abstracts the different kinds of backing memory (MMAP, USERPTR, DMABUF)
//! supported by V4L2.
mod dmabuf;
mod mmap;
mod userptr;

pub use dmabuf::*;
pub use mmap::*;
pub use userptr::*;

use crate::bindings;
use std::fmt::Debug;

#[derive(Debug, Clone, Copy)]
pub enum MemoryType {
    MMAP = bindings::v4l2_memory_V4L2_MEMORY_MMAP as isize,
    UserPtr = bindings::v4l2_memory_V4L2_MEMORY_USERPTR as isize,
    DMABuf = bindings::v4l2_memory_V4L2_MEMORY_DMABUF as isize,
}

/// Trait for handles that point to actual buffer data. Each one of the `MMAP`,
/// `UserPtr`, and `DMABuf` memory types have a handler implementation, used
/// with the `ioctl` module.
pub trait PlaneHandle: Sized + Debug {
    /// The memory type that this handle backs.
    const MEMORY_TYPE: MemoryType;

    /// Move the plane information into the buffer (for single-planar queues).
    fn fill_v4l2_buffer(plane: &bindings::v4l2_plane, buffer: &mut bindings::v4l2_buffer);
    /// Fill a plane of a multi-planar V4L2 buffer with the handle's information.
    fn fill_v4l2_plane(&self, plane: &mut bindings::v4l2_plane);
}

/// Trait for a memory type to be used with the `device` module. There are three
/// memory types: `MMAP`, `UserPtr`, and `DMABuf`, which all implement this
/// trait in order to abstract their differences. Methods and structures dealing
/// with buffers are also generally typed after this trait.
pub trait Memory {
    /// The type of handle produced by this memory type, for use with the
    /// `ioctl` module.
    type HandleType: PlaneHandle;
}

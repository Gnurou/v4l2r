//! Abstracts the different kinds of backing memory (MMAP, USERPTR, DMABUF)
//! supported by V4L2.
mod dmabuf;
mod mmap;
mod userptr;

pub use dmabuf::*;
pub use mmap::*;
pub use userptr::*;

use crate::{
    bindings,
    ioctl::{PlaneMapping, QueryBufPlane},
};
use std::fmt::Debug;
use std::os::unix::io::AsRawFd;

#[derive(Debug, Clone, Copy)]
pub enum MemoryType {
    MMAP = bindings::v4l2_memory_V4L2_MEMORY_MMAP as isize,
    UserPtr = bindings::v4l2_memory_V4L2_MEMORY_USERPTR as isize,
    DMABuf = bindings::v4l2_memory_V4L2_MEMORY_DMABUF as isize,
}

/// Trait for handles that point to actual buffer data. Each one of the `MMAP`,
/// `UserPtr`, and `DMABuf` memory types have a handler implementation, used
/// with the `ioctl` module.
pub trait PlaneHandle: Debug + Send {
    /// The memory type that this handle attaches to.
    const MEMORY_TYPE: MemoryType;

    /// Move the plane information into the buffer (for single-planar queues).
    fn fill_v4l2_buffer(plane: &bindings::v4l2_plane, buffer: &mut bindings::v4l2_buffer);
    /// Fill a plane of a multi-planar V4L2 buffer with the handle's information.
    fn fill_v4l2_plane(&self, plane: &mut bindings::v4l2_plane);
}

// Trait describing a memory type that can be mapped from a V4L2 buffer's information.
pub trait Mappable {
    fn map<D: AsRawFd>(device: &D, plane_info: &QueryBufPlane) -> Option<PlaneMapping>;
}

/// Trait describing a memory type that can be used to back V4L2 buffers.
pub trait Memory: 'static {
    // A type that can be applied into a v4l2_plane or v4l2_buffer.
    type HandleType: PlaneHandle;
}

/// Contains the handles (pointers to user memory or DMABUFs) that are kept
/// when a buffer is processed by the kernel and returned to the user upon
/// `dequeue()` or `streamoff()`.
#[allow(type_alias_bounds)]
pub type PlaneHandles<M: Memory> = Vec<M::HandleType>;

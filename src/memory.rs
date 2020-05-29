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

    /// Fill a single-planar V4L2 buffer with the handle's information.
    fn fill_v4l2_buffer(&self, buffer: &mut bindings::v4l2_buffer);
    // Fill a plane of a multi-planar V4L2 buffer with the handle's information.
    fn fill_v4l2_plane(&self, plane: &mut bindings::v4l2_plane);
}

/// Trait for a memory type to be used with the `device` module. There are three
/// memory types: `MMAP`, `UserPtr`, and `DMABuf`, which all implement this
/// trait in order to abstract their differences. Methods and structures dealing
/// with buffers are also generally typed after this trait.
pub trait Memory {
    /// The type we need to pass to enqueue a buffer.
    type QBufType: Send;

    /// The type we will get back when dequeuing a buffer, or when calling
    /// `streamoff` on the queue.
    type DQBufType: Send;

    /// The type of handle produced by this memory type, for use with the
    /// `ioctl` module.
    type HandleType: PlaneHandle;

    /// Builds a handle to give to `qbuf` from the `QBufType` we have been given.
    ///
    /// This method is unsafe because the caller must guarantee that the owned
    /// data passed as `qb` will outlive the returned handle type and will also
    /// not move.
    ///
    /// This is not a problem for MMAP buffers, because there is no handle and
    /// the buffer data is owned by the kernel. However the following rules apply
    /// for other buffer types:
    ///
    /// * For USERPTR buffers, the caller must make sure that the owned buffer
    ///   data will outlive the created handle. In this case, the handle's
    ///   lifetime includes the time it spends in the kernel (i.e. between
    ///   being queued and dequeued (or the queue being streamed off).
    /// * For DMABUF buffers, the caller must make sure that the passed fd
    ///   will not be closed before the handle is given to the kernel.
    ///
    /// The simplest way to achieve this is for the caller to keep owning `qb`
    /// until the buffer is dequeued, regardless of the buffer type, but shorter
    /// lifecycles are allowed is the buffer type is known in advance and permits
    /// it.
    unsafe fn build_handle(qb: &Self::QBufType) -> Self::HandleType;

    /// Turn the `QBufType` we were given when queueing into the structure
    /// we want to give back to the client.
    fn build_dqbuftype(qb: Self::QBufType) -> Self::DQBufType;
}

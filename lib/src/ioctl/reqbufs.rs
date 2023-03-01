//! Safe wrapper for the `VIDIOC_REQBUFS` ioctl.
use crate::bindings;
use crate::memory::MemoryType;
use crate::QueueType;
use bitflags::bitflags;
use nix::{self, errno::Errno};
use std::mem;
use std::os::unix::io::AsRawFd;
use thiserror::Error;

/// Implementors can receive the result from the `reqbufs` ioctl.
pub trait ReqBufs {
    fn from(reqbufs: bindings::v4l2_requestbuffers) -> Self;
}

impl ReqBufs for bindings::v4l2_requestbuffers {
    fn from(reqbufs: bindings::v4l2_requestbuffers) -> Self {
        reqbufs
    }
}

bitflags! {
    /// Flags returned by the `VIDIOC_REQBUFS` ioctl into the `capabilities`
    /// field of `struct v4l2_requestbuffers`.
    pub struct BufferCapabilities: u32 {
        const SUPPORTS_MMAP = bindings::V4L2_BUF_CAP_SUPPORTS_MMAP;
        const SUPPORTS_USERPTR = bindings::V4L2_BUF_CAP_SUPPORTS_USERPTR;
        const SUPPORTS_DMABUF = bindings::V4L2_BUF_CAP_SUPPORTS_DMABUF;
        const SUPPORTS_REQUESTS = bindings::V4L2_BUF_CAP_SUPPORTS_REQUESTS;
        const SUPPORTS_ORPHANED_BUFS = bindings::V4L2_BUF_CAP_SUPPORTS_ORPHANED_BUFS;
        //const SUPPORTS_M2M_HOLD_CAPTURE_BUF = bindings::V4L2_BUF_CAP_SUPPORTS_M2M_HOLD_CAPTURE_BUF;
    }
}

impl ReqBufs for () {
    fn from(_reqbufs: bindings::v4l2_requestbuffers) -> Self {}
}

/// In case we are just interested in the number of buffers that `reqbufs`
/// created.
impl ReqBufs for usize {
    fn from(reqbufs: bindings::v4l2_requestbuffers) -> Self {
        reqbufs.count as usize
    }
}

/// If we just want to query the buffer capabilities.
impl ReqBufs for BufferCapabilities {
    fn from(reqbufs: bindings::v4l2_requestbuffers) -> Self {
        BufferCapabilities::from_bits_truncate(reqbufs.capabilities)
    }
}

/// Full result of the `reqbufs` ioctl.
pub struct RequestBuffers {
    pub count: u32,
    pub capabilities: BufferCapabilities,
}

impl ReqBufs for RequestBuffers {
    fn from(reqbufs: bindings::v4l2_requestbuffers) -> Self {
        RequestBuffers {
            count: reqbufs.count,
            capabilities: BufferCapabilities::from_bits_truncate(reqbufs.capabilities),
        }
    }
}

#[doc(hidden)]
mod ioctl {
    use crate::bindings::v4l2_requestbuffers;
    nix::ioctl_readwrite!(vidioc_reqbufs, b'V', 8, v4l2_requestbuffers);
}

#[derive(Debug, Error)]
pub enum ReqbufsError {
    #[error("invalid buffer ({0}) or memory type ({1:?}) requested")]
    InvalidBufferType(QueueType, MemoryType),
    #[error("ioctl error: {0}")]
    IoctlError(nix::Error),
}

impl From<ReqbufsError> for Errno {
    fn from(err: ReqbufsError) -> Self {
        match err {
            ReqbufsError::InvalidBufferType(_, _) => Errno::EINVAL,
            ReqbufsError::IoctlError(e) => e,
        }
    }
}

/// Safe wrapper around the `VIDIOC_REQBUFS` ioctl.
pub fn reqbufs<T: ReqBufs>(
    fd: &impl AsRawFd,
    queue: QueueType,
    memory: MemoryType,
    count: u32,
) -> Result<T, ReqbufsError> {
    let mut reqbufs = bindings::v4l2_requestbuffers {
        count,
        type_: queue as u32,
        memory: memory as u32,
        ..unsafe { mem::zeroed() }
    };

    match unsafe { ioctl::vidioc_reqbufs(fd.as_raw_fd(), &mut reqbufs) } {
        Ok(_) => Ok(T::from(reqbufs)),
        Err(Errno::EINVAL) => Err(ReqbufsError::InvalidBufferType(queue, memory)),
        Err(e) => Err(ReqbufsError::IoctlError(e)),
    }
}

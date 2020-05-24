//! Operations specific to DMABuf-type buffers.
use super::*;
use crate::bindings;
use std::default::Default;
use std::fs::File;
use std::os::unix::io::{AsRawFd, RawFd};

/// Handle for a DMABUF buffer. These buffers are backed by DMABuf-shared
/// memory.
#[derive(Debug, Default)]
pub struct DMABufHandle(RawFd);

impl DMABufHandle {
    /// Create a new DMABUF handle.
    ///
    /// This method is unsafe. The caller must ensure that `fd` will not be closed
    /// at least until the buffer using this handle is queued.
    unsafe fn new(fd: RawFd) -> Self {
        DMABufHandle(fd)
    }
}

impl PlaneHandle for DMABufHandle {
    const MEMORY_TYPE: MemoryType = MemoryType::DMABuf;

    fn fill_v4l2_buffer(&self, buffer: &mut bindings::v4l2_buffer) {
        buffer.m.fd = self.0;
    }

    fn fill_v4l2_plane(&self, plane: &mut bindings::v4l2_plane) {
        plane.m.fd = self.0;
    }
}

pub struct DMABuf;

/// DMABUF buffers support for queues. This takes a `File` containing the
/// DMABUF file description as `qbuf` input, and gives it back as output so it
/// can be reused.
///
/// TODO Reusing the same DMABUF on the save V4L2 buffer saves some processing
/// in the kernel, so maybe some binding or other affinity should be done?
impl Memory for DMABuf {
    type QBufType = File;
    type DQBufType = Self::QBufType;
    type HandleType = DMABufHandle;

    unsafe fn build_handle(qb: &Self::QBufType) -> Self::HandleType {
        DMABufHandle::new(qb.as_raw_fd())
    }

    fn build_dqbuftype(qb: Self::QBufType) -> Self::DQBufType {
        qb
    }
}

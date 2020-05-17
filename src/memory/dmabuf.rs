//! Operations specific to DMABuf-type buffers.
use super::*;
use crate::bindings;
use std::default::Default;
use std::os::unix::io::RawFd;

/// Handle for a DMABUF buffer. These buffers are backed by DMABuf-shared
/// memory.
#[derive(Debug, Default)]
pub struct DMABufHandle(RawFd);

impl DMABufHandle {
    /// Create a new DMABUF handle.
    ///
    /// This method is unsafe. The caller must ensure that `fd` will not be closed
    /// at least until the buffer using this handle is queued.
    pub unsafe fn new(fd: RawFd) -> Self {
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

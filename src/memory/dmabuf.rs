//! Operations specific to DMABuf-type buffers.
use crate::bindings;
use crate::{MemoryType, PlaneHandle};
use std::default::Default;
use std::os::unix::io::RawFd;

/// Handle for a DMABUF buffer. These buffers are backed by DMABuf-shared
/// memory.
#[derive(Debug, Default)]
pub struct DMABufHandle(RawFd);

impl DMABufHandle {
    pub fn new(fd: RawFd) -> Self {
        DMABufHandle(fd)
    }
}

impl PlaneHandle for DMABufHandle {
    const MEMORY_TYPE: MemoryType = MemoryType::DMABuf;

    unsafe fn from_v4l2_buffer(buffer: &bindings::v4l2_buffer) -> Self {
        DMABufHandle(buffer.m.fd)
    }

    unsafe fn from_v4l2_plane(plane: &bindings::v4l2_plane) -> Self {
        DMABufHandle(plane.m.fd)
    }

    fn fill_v4l2_buffer(&self, buffer: &mut bindings::v4l2_buffer) {
        buffer.m.fd = self.0;
    }

    fn fill_v4l2_plane(&self, plane: &mut bindings::v4l2_plane) {
        plane.m.fd = self.0;
    }
}

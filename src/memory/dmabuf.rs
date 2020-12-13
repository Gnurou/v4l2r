//! Operations specific to DMABuf-type buffers.
use super::*;
use crate::bindings;
use std::os::unix::io::AsRawFd;

pub struct DMABuf;

impl Memory for DMABuf {
    const MEMORY_TYPE: MemoryType = MemoryType::DMABuf;
}

impl Imported for DMABuf {}

/// Handle for a DMABUF plane. Any type that can provide a file descriptor is
/// valid.
#[derive(Debug)]
pub struct DMABufHandle<T: AsRawFd + Debug + Send>(pub T);

impl<T: AsRawFd + Debug + Send> From<T> for DMABufHandle<T> {
    fn from(dmabuf: T) -> Self {
        DMABufHandle(dmabuf)
    }
}

impl<T: AsRawFd + Debug + Send> PlaneHandle for DMABufHandle<T> {
    type Memory = DMABuf;

    fn fill_v4l2_plane(&self, plane: &mut bindings::v4l2_plane) {
        plane.m.fd = self.0.as_raw_fd();
    }

    fn fill_v4l2_splane_buffer(plane: &bindings::v4l2_plane, buffer: &mut bindings::v4l2_buffer) {
        buffer.m.fd = unsafe { plane.m.fd };
    }
}

//! Operations specific to DMABuf-type buffers.
use super::*;
use crate::bindings;
use std::fs::File;
use std::os::unix::io::AsRawFd;

#[derive(Debug)]
pub struct Fd(File);

impl PlaneHandle for Fd {
    const MEMORY_TYPE: MemoryType = MemoryType::DMABuf;

    fn fill_v4l2_buffer(plane: &bindings::v4l2_plane, buffer: &mut bindings::v4l2_buffer) {
        buffer.m.fd = unsafe { plane.m.fd };
    }

    fn fill_v4l2_plane(&self, plane: &mut bindings::v4l2_plane) {
        plane.m.fd = self.0.as_raw_fd();
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
    type HandleType = Fd;
    type Type = Dynamic;
    type MapperType = ();
}

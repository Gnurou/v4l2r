//! Operations specific to DMABuf-type buffers.
use super::*;
use crate::bindings;
use std::fs::File;
use std::os::unix::io::{AsRawFd, RawFd};

type DMABufHandle = RawFd;

impl PlaneHandle for DMABufHandle {
    const MEMORY_TYPE: MemoryType = MemoryType::DMABuf;

    fn fill_v4l2_buffer(&self, buffer: &mut bindings::v4l2_buffer) {
        buffer.m.fd = *self;
    }

    fn fill_v4l2_plane(&self, plane: &mut bindings::v4l2_plane) {
        plane.m.fd = *self;
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
        qb.as_raw_fd()
    }

    fn build_dqbuftype(qb: Self::QBufType) -> Self::DQBufType {
        qb
    }
}

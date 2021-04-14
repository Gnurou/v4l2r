//! Operations specific to DMABuf-type buffers.
use log::warn;

use super::*;
use crate::{bindings, ioctl};
use std::os::unix::io::AsRawFd;

pub struct DmaBuf;

pub type DmaBufferHandles<T> = Vec<DmaBufHandle<T>>;

impl Memory for DmaBuf {
    const MEMORY_TYPE: MemoryType = MemoryType::DmaBuf;
}

impl Imported for DmaBuf {}

pub trait DmaBufSource: AsRawFd + Debug + Send {
    fn len(&self) -> u64;

    /// Make Clippy happy.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl DmaBufSource for std::fs::File {
    fn len(&self) -> u64 {
        match self.metadata() {
            Err(_) => {
                warn!("Failed to compute File size for use as DMABuf, using 0...");
                0
            }
            Ok(m) => m.len(),
        }
    }
}

/// Handle for a DMABUF plane. Any type that can provide a file descriptor is
/// valid.
#[derive(Debug)]
pub struct DmaBufHandle<T: DmaBufSource>(pub T);

impl<T: DmaBufSource> From<T> for DmaBufHandle<T> {
    fn from(dmabuf: T) -> Self {
        DmaBufHandle(dmabuf)
    }
}

impl<T: DmaBufSource + 'static> PlaneHandle for DmaBufHandle<T> {
    type Memory = DmaBuf;

    fn fill_v4l2_plane(&self, plane: &mut bindings::v4l2_plane) {
        plane.m.fd = self.0.as_raw_fd();
        plane.length = self.0.len() as u32;
    }
}

impl<T: DmaBufSource> DmaBufHandle<T> {
    pub fn map(&self) -> Result<PlaneMapping, ioctl::MmapError> {
        let len = self.0.len();

        ioctl::mmap(&self.0, 0, len as u32)
    }
}

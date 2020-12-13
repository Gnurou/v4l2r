//! Operations specific to MMAP-type buffers.
use super::*;
use crate::{bindings, ioctl, Format};
use std::fmt::Debug;

#[derive(Default)]
pub struct MMAP;

impl Memory for MMAP {
    const MEMORY_TYPE: MemoryType = MemoryType::MMAP;
}

impl SelfBacked for MMAP {}

/// Dummy handle for a MMAP plane, to use with APIs that require handles. MMAP
/// buffers are backed by the device, and thus we don't need to attach any extra
/// information to them.
#[derive(Default, Debug, Clone)]
pub struct MMAPHandle;

// There is no information to fill with MMAP buffers ; the index is enough.
impl PlaneHandle for MMAPHandle {
    type Memory = MMAP;

    fn fill_v4l2_plane(&self, _plane: &mut bindings::v4l2_plane) {}
    fn fill_v4l2_splane_buffer(_plane: &bindings::v4l2_plane, _buffer: &mut bindings::v4l2_buffer) {
    }
}

impl Mappable for MMAPHandle {
    fn map<D: AsRawFd>(device: &D, plane_info: &QueryBufPlane) -> Option<PlaneMapping> {
        Some(ioctl::mmap(device, plane_info.mem_offset, plane_info.length).ok()?)
    }
}

pub struct MMAPProvider(Vec<MMAPHandle>);

impl MMAPProvider {
    pub fn new(format: &Format) -> Self {
        Self(vec![Default::default(); format.plane_fmt.len()])
    }
}

impl super::HandlesProvider for MMAPProvider {
    type HandleType = Vec<MMAPHandle>;

    fn get_handles(&mut self) -> Option<Self::HandleType> {
        Some(self.0.clone())
    }
}

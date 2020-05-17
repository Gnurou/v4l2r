use super::{BufferStateFuse, PlaneHandles};
use crate::ioctl;
use crate::memory::Memory;

/// Represents the information of a dequeued buffer.
pub struct DQBuffer<M: Memory> {
    /// The backing memory that has been provided for this buffer. Only useful
    /// if the buffers are of USERPTR type.
    pub plane_handles: PlaneHandles<M>,
    /// Dequeued buffer information as reported by V4L2.
    pub data: ioctl::DQBuffer,
    /// Fuse that will put the buffer back into the `Free` state when this
    /// object is destroyed.
    _fuse: BufferStateFuse<M>,
}

impl<M: Memory> DQBuffer<M> {
    pub(super) fn new(
        plane_handles: PlaneHandles<M>,
        data: ioctl::DQBuffer,
        fuse: BufferStateFuse<M>,
    ) -> Self {
        DQBuffer {
            plane_handles,
            data,
            _fuse: fuse,
        }
    }
}

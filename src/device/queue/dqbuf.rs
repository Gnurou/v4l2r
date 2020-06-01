//! Provides types related to dequeuing buffers from a `Queue` object.
use super::{BufferStateFuse, PlaneHandles};
use crate::ioctl;
use crate::memory::Memory;

/// Represents the information of a dequeued buffer. This is basically the same
/// information as what the `ioctl` interface provides, but it also includes
/// the plane handles that have been provided when the buffer was queued to
/// return their ownership to the user.
pub struct DQBuffer<M: Memory> {
    /// The backing memory that has been provided for this buffer. Only useful
    /// if the buffers are of USERPTR type.
    pub plane_handles: PlaneHandles<M>,
    /// Dequeued buffer information as reported by V4L2.
    pub data: ioctl::DQBuffer,
    /// Callback to be run when the object is dropped.
    drop_callback: Option<Box<dyn FnOnce(&mut Self) + Send>>,
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
            drop_callback: None,
        }
    }

    /// Attach a callback that will be called when the DQBuffer is destroyed,
    /// and after the buffer has been returned to the free list.
    pub fn set_drop_callback<F: FnOnce(&mut Self) + Send + 'static>(&mut self, callback: F) {
        self.drop_callback = Some(Box::new(callback));
    }
}

impl<M: Memory> Drop for DQBuffer<M> {
    fn drop(&mut self) {
        // Make sure the buffer is returned to the free state before we call
        // the callback.
        self._fuse.trigger();
        if let Some(callback) = self.drop_callback.take() {
            callback(self);
        }
    }
}

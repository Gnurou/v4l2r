//! Provides types related to dequeuing buffers from a `Queue` object.
use super::{
    direction::{Capture, Direction},
    states::BuffersAllocated,
    BufferStateFuse, PlaneHandles, Queue,
};
use crate::ioctl;
use crate::memory::Memory;
use crate::{device::Device, memory::Mappable};
use ioctl::{PlaneMapping, QueryBuffer};
use std::sync::{Arc, Weak};

/// Represents the information of a dequeued buffer. This is basically the same
/// information as what the `ioctl` interface provides, but it also includes
/// the plane handles that have been provided when the buffer was queued to
/// return their ownership to the user.
pub struct DQBuffer<D: Direction, M: Memory> {
    /// The backing memory that has been provided for this buffer. Only useful
    /// if the buffers are of USERPTR type.
    pub plane_handles: PlaneHandles<M>,
    /// Dequeued buffer information as reported by V4L2.
    pub data: ioctl::DQBuffer,
    device: Weak<Device>,
    buffer_info: Weak<QueryBuffer>,
    /// Callback to be run when the object is dropped.
    drop_callback: Option<Box<dyn FnOnce(&mut Self) + Send>>,
    /// Fuse that will put the buffer back into the `Free` state when this
    /// object is destroyed.
    fuse: BufferStateFuse<M>,
    _d: std::marker::PhantomData<D>,
}

impl<D: Direction, M: Memory> DQBuffer<D, M> {
    pub(super) fn new(
        queue: &Queue<D, BuffersAllocated<M>>,
        buffer: &Arc<QueryBuffer>,
        plane_handles: PlaneHandles<M>,
        data: ioctl::DQBuffer,
        fuse: BufferStateFuse<M>,
    ) -> Self {
        DQBuffer {
            plane_handles,
            data,
            device: Arc::downgrade(&queue.inner.device),
            buffer_info: Arc::downgrade(buffer),
            fuse,
            drop_callback: None,
            _d: std::marker::PhantomData,
        }
    }

    /// Attach a callback that will be called when the DQBuffer is destroyed,
    /// and after the buffer has been returned to the free list.
    pub fn set_drop_callback<F: FnOnce(&mut Self) + Send + 'static>(&mut self, callback: F) {
        self.drop_callback = Some(Box::new(callback));
    }
}

impl<M: Memory + Mappable> DQBuffer<Capture, M> {
    // TODO returned mapping should be read-only!
    // TODO only return a bytes_used slice?
    pub fn get_plane_mapping(&self, plane: usize) -> Option<PlaneMapping> {
        // We can only obtain a mapping if this buffer has not been deleted.
        let buffer_info = self.buffer_info.upgrade()?;
        let plane = buffer_info.planes.get(plane)?;
        // If the buffer info was alive, then the device must also be.
        let device = self.device.upgrade()?;

        M::map(device.as_ref(), plane)
    }
}

impl<D: Direction, M: Memory> Drop for DQBuffer<D, M> {
    fn drop(&mut self) {
        // Make sure the buffer is returned to the free state before we call
        // the callback.
        self.fuse.trigger();
        if let Some(callback) = self.drop_callback.take() {
            callback(self);
        }
    }
}

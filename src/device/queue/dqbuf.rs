//! Provides types related to dequeuing buffers from a `Queue` object.
use super::{
    direction::{Capture, Direction},
    BufferStateFuse, BuffersAllocated, PlaneHandles, Queue,
};
use crate::ioctl;
use crate::memory::Memory;
use crate::{device::Device, memory::Mappable};
use ioctl::{PlaneMapping, QueryBuffer};
use std::{
    fmt::Debug,
    sync::{Arc, Weak},
};

/// Represents the information of a dequeued buffer. This is basically the same
/// information as what the `ioctl` interface provides, but it also includes
/// the plane handles that have been provided when the buffer was queued to
/// return their ownership to the user.
pub struct DQBuffer<D: Direction, M: Memory> {
    /// Dequeued buffer information as reported by V4L2.
    pub data: ioctl::DQBuffer,
    /// The backing memory that has been provided for this buffer. Only useful
    /// if the buffers are of USERPTR type.
    pub plane_handles: PlaneHandles<M>,

    device: Weak<Device>,
    buffer_info: Weak<QueryBuffer>,
    /// Callbacks to be run when the object is dropped.
    drop_callbacks: Vec<Box<dyn FnOnce(&mut Self) + Send>>,
    /// Fuse that will put the buffer back into the `Free` state when this
    /// object is destroyed.
    fuse: BufferStateFuse<M>,
    _d: std::marker::PhantomData<D>,
}

impl<D: Direction, M: Memory> Debug for DQBuffer<D, M> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        self.data.fmt(f)
    }
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
            drop_callbacks: Default::default(),
            _d: std::marker::PhantomData,
        }
    }

    /// Attach a callback that will be called when the DQBuffer is destroyed,
    /// and after the buffer has been returned to the free list.
    /// This method can be called several times, the callback will be run in
    /// the inverse order that they were added.
    pub fn add_drop_callback<F: FnOnce(&mut Self) + Send + 'static>(&mut self, callback: F) {
        self.drop_callbacks.push(Box::new(callback));
    }
}

impl<M: Memory + Mappable> DQBuffer<Capture, M> {
    // TODO returned mapping should be read-only!
    pub fn get_plane_mapping(&self, plane_index: usize) -> Option<PlaneMapping> {
        // We can only obtain a mapping if this buffer has not been deleted.
        let buffer_info = self.buffer_info.upgrade()?;
        let plane = buffer_info.planes.get(plane_index)?;
        let plane_data = self.data.planes.get(plane_index)?;
        // If the buffer info was alive, then the device must also be.
        let device = self.device.upgrade()?;

        let start = plane_data.data_offset as usize;
        let end = start + plane_data.bytesused as usize;

        Some(M::map(device.as_ref(), plane)?.restrict(start, end))
    }
}

impl<D: Direction, M: Memory> Drop for DQBuffer<D, M> {
    fn drop(&mut self) {
        // Make sure the buffer is returned to the free state before we call
        // the callbacks.
        self.fuse.trigger();
        while let Some(callback) = self.drop_callbacks.pop() {
            callback(self);
        }
    }
}

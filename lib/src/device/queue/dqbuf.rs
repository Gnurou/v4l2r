//! Provides types related to dequeuing buffers from a `Queue` object.
use super::{
    buffer::BufferInfo,
    direction::{Capture, Direction},
    BufferStateFuse, BuffersAllocated, Queue,
};
use crate::ioctl::{self, PlaneMapping};
use crate::{
    device::Device,
    memory::{BufferHandles, Mappable, PrimitiveBufferHandles},
};
use std::{
    fmt::Debug,
    sync::{Arc, Weak},
};

pub type DropCallback<D, P> = Box<dyn FnOnce(&mut DqBuffer<D, P>) + Send>;

/// Represents the information of a dequeued buffer. This is basically the same
/// information as what the `ioctl` interface provides, but it also includes
/// the plane handles that have been provided when the buffer was queued to
/// return their ownership to the user.
pub struct DqBuffer<D: Direction, P: BufferHandles> {
    /// Dequeued buffer information as reported by V4L2.
    pub data: ioctl::V4l2Buffer,
    /// The backing memory that has been provided for this buffer.
    plane_handles: Option<P>,

    device: Weak<Device>,
    buffer_info: Weak<BufferInfo<P>>,
    /// Callbacks to be run when the object is dropped.
    drop_callbacks: Vec<DropCallback<D, P>>,
    /// Fuse that will put the buffer back into the `Free` state when this
    /// object is destroyed.
    fuse: BufferStateFuse<P>,
    _d: std::marker::PhantomData<D>,
}

impl<D: Direction, P: BufferHandles> Debug for DqBuffer<D, P> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        self.data.fmt(f)
    }
}

impl<D: Direction, P: BufferHandles> DqBuffer<D, P> {
    pub(super) fn new(
        queue: &Queue<D, BuffersAllocated<P>>,
        buffer: &Arc<BufferInfo<P>>,
        plane_handles: P,
        data: ioctl::V4l2Buffer,
        fuse: BufferStateFuse<P>,
    ) -> Self {
        DqBuffer {
            plane_handles: Some(plane_handles),
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

    /// Return the plane handles of the buffer. This method is guaranteed to
    /// return Some() the first time it is called, and None any subsequent times.
    pub fn take_handles(&mut self) -> Option<P> {
        self.plane_handles.take()
    }
}

impl<P> DqBuffer<Capture, P>
where
    P: PrimitiveBufferHandles,
    P::HandleType: Mappable,
{
    // TODO returned mapping should be read-only!
    pub fn get_plane_mapping(&self, plane_index: usize) -> Option<PlaneMapping> {
        // We can only obtain a mapping if this buffer has not been deleted.
        let buffer_info = self.buffer_info.upgrade()?;
        let plane = buffer_info.features.planes.get(plane_index)?;
        let plane_data = self.data.get_plane(plane_index)?;
        // If the buffer info was alive, then the device must also be.
        let device = self.device.upgrade()?;

        let start = plane_data.data_offset() as usize;
        let end = start + plane_data.bytesused() as usize;

        Some(P::HandleType::map(device.as_ref(), plane)?.restrict(start, end))
    }
}

impl<D: Direction, P: BufferHandles> Drop for DqBuffer<D, P> {
    fn drop(&mut self) {
        // Make sure the buffer is returned to the free state before we call
        // the callbacks.
        self.fuse.trigger();
        while let Some(callback) = self.drop_callbacks.pop() {
            callback(self);
        }
    }
}

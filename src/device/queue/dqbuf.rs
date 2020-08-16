//! Provides types related to dequeuing buffers from a `Queue` object.
use super::{
    direction::{Capture, Direction},
    states::BuffersAllocated,
    BufferStateFuse, PlaneHandles, Queue,
};
use crate::ioctl;
use crate::memory::Memory;
use crate::memory::{Fixed, PlaneMapper};
use ioctl::{PlaneMapping, QueryBuffer};

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
    /// Callback to be run when the object is dropped.
    drop_callback: Option<Box<dyn FnOnce(&mut Self) + Send>>,
    /// Mapper objects for fixed memory types.
    plane_maps: Vec<M::MapperType>,
    /// Fuse that will put the buffer back into the `Free` state when this
    /// object is destroyed.
    fuse: BufferStateFuse<M>,
    _d: std::marker::PhantomData<D>,
}

impl<D: Direction, M: Memory> DQBuffer<D, M> {
    pub(super) fn new(
        queue: &Queue<D, BuffersAllocated<M>>,
        buffer: &QueryBuffer,
        plane_handles: PlaneHandles<M>,
        data: ioctl::DQBuffer,
        fuse: BufferStateFuse<M>,
    ) -> Self {
        let plane_maps = buffer
            .planes
            .iter()
            .map(|p| M::MapperType::new(&queue.inner.device, p.mem_offset, p.length))
            .collect();

        DQBuffer {
            plane_handles,
            data,
            fuse,
            drop_callback: None,
            plane_maps,
            _d: std::marker::PhantomData,
        }
    }

    /// Attach a callback that will be called when the DQBuffer is destroyed,
    /// and after the buffer has been returned to the free list.
    pub fn set_drop_callback<F: FnOnce(&mut Self) + Send + 'static>(&mut self, callback: F) {
        self.drop_callback = Some(Box::new(callback));
    }
}

impl<M: Memory<Type = Fixed>> DQBuffer<Capture, M> {
    // TODO returned mapping should be read-only!
    // TODO only return a bytes_used slice?
    pub fn get_plane_mapping(&self, plane: usize) -> Option<PlaneMapping> {
        let mapper = self.plane_maps.get(plane)?;
        mapper.map()
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

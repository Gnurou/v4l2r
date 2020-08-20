pub mod direction;
pub mod dqbuf;
pub mod qbuf;
pub mod states;

use super::Device;
use crate::ioctl;
use crate::memory::*;
use crate::*;
use direction::*;
use dqbuf::*;
use ioctl::QueryBuffer;
use qbuf::*;
use states::BufferState;
use states::*;
use std::os::unix::io::{AsRawFd, RawFd};
use std::sync::{Arc, Mutex, Weak};

/// Contains the handles (pointers to user memory or DMABUFs) that are kept
/// when a buffer is processed by the kernel and returned to the user upon
/// `dequeue()` or `streamoff()`.
#[allow(type_alias_bounds)]
pub type PlaneHandles<M: Memory> = Vec<M::HandleType>;

/// Base values of a queue, that are always value no matter the state the queue
/// is in. This base object remains alive as long as the queue is borrowed from
/// the `Device`.
pub struct QueueBase {
    /// Reference to the device, so `fd` is kept valid and to let us mark the
    /// queue as free again upon destruction.
    device: Arc<Device>,
    type_: QueueType,
    capabilities: ioctl::BufferCapabilities,
}

impl AsRawFd for QueueBase {
    fn as_raw_fd(&self) -> RawFd {
        self.device.as_raw_fd()
    }
}

impl<'a> Drop for QueueBase {
    /// Make the queue available again.
    fn drop(&mut self) {
        assert_eq!(
            self.device.used_queues.lock().unwrap().remove(&self.type_),
            true
        );
    }
}

/// V4L2 queue object. Specialized according to its configuration state so that
/// only valid methods can be called from a given point.
pub struct Queue<D, S>
where
    D: Direction,
    S: QueueState,
{
    inner: QueueBase,
    _d: std::marker::PhantomData<D>,
    state: S,
}

/// Methods of `Queue` that are available no matter the state.
impl<D, S> Queue<D, S>
where
    D: Direction,
    S: QueueState,
{
    pub fn get_capabilities(&self) -> ioctl::BufferCapabilities {
        self.inner.capabilities
    }

    pub fn get_type(&self) -> QueueType {
        self.inner.type_
    }

    pub fn get_format(&self) -> Result<Format> {
        ioctl::g_fmt(&self.inner, self.inner.type_)
    }

    /// This method can invalidate any current format iterator, hence it requires
    /// the queue to be mutable. This way of doing is not perfect though, as setting
    /// the format on one queue can change the options available on another.
    pub fn set_format(&mut self, format: Format) -> Result<Format> {
        let type_ = self.inner.type_;
        ioctl::s_fmt(&mut self.inner, type_, format)
    }

    /// Performs exactly as `set_format`, but does not actually apply `format`.
    /// Useful to check what modifications need to be done to a format before it
    /// can be used.
    pub fn try_format(&self, format: Format) -> Result<Format> {
        ioctl::try_fmt(&self.inner, self.inner.type_, format)
    }

    /// Returns a `FormatBuilder` which is set to the currently active format
    /// and can be modified and eventually applied. The `FormatBuilder` holds
    /// a mutable reference to this `Queue`.
    pub fn change_format<'a>(&'a mut self) -> Result<FormatBuilder<'a>> {
        FormatBuilder::new(&mut self.inner)
    }

    /// Returns an iterator over all the formats currently supported by this queue.
    pub fn format_iter(&self) -> ioctl::FormatIterator<QueueBase> {
        ioctl::FormatIterator::new(&self.inner, self.inner.type_)
    }
}

/// Builder for a V4L2 format. This takes a mutable reference on the queue, so
/// it is supposed to be short-lived: get one, adjust the format, and apply.
pub struct FormatBuilder<'a> {
    queue: &'a mut QueueBase,
    format: Format,
}

impl<'a> FormatBuilder<'a> {
    fn new(queue: &'a mut QueueBase) -> Result<Self> {
        let format = ioctl::g_fmt(queue, queue.type_)?;
        Ok(Self { queue, format })
    }

    /// Get a reference to the format built so far. Useful for checking the
    /// currently set format after getting a builder, or the actual settings
    /// that will be applied by the kernel after a `try_apply()`.
    pub fn format(&self) -> &Format {
        &self.format
    }

    pub fn set_size(mut self, width: usize, height: usize) -> Self {
        self.format.width = width as u32;
        self.format.height = height as u32;
        self
    }

    pub fn set_pixelformat(mut self, pixel_format: impl Into<PixelFormat>) -> Self {
        self.format.pixelformat = pixel_format.into();
        self
    }

    /// Apply the format built so far. The kernel will adjust the format to fit
    /// the driver's capabilities if needed, and the format actually applied will
    /// be returned.
    pub fn apply(self) -> Result<Format> {
        ioctl::s_fmt(self.queue, self.queue.type_, self.format)
    }

    /// Try to apply the format built so far. The kernel will adjust the format
    /// to fit the driver's capabilities if needed, so make sure to check important
    /// parameters upon return.
    ///
    /// Calling `apply()` right after this method is guaranteed to successfully
    /// apply the format without further change.
    pub fn try_apply(&mut self) -> Result<()> {
        let new_format = ioctl::try_fmt(self.queue, self.queue.type_, self.format.clone())?;

        self.format = new_format;
        Ok(())
    }
}

impl<D: Direction> Queue<D, QueueInit> {
    /// Create a queue for type `queue_type` on `device`. A queue of a specific type
    /// can be requested only once.
    ///
    /// Not all devices support all kinds of queue. To test whether the queue is supported,
    /// a REQBUFS(0) is issued on the device. If it is not successful, the device is
    /// deemed to not support this kind of queue and this method will fail.
    fn create(device: Arc<Device>, queue_type: QueueType) -> Result<Queue<D, QueueInit>> {
        let mut used_queues = device.used_queues.lock().unwrap();

        if used_queues.contains(&queue_type) {
            return Err(Error::AlreadyBorrowed);
        }

        // Check that the queue is valid for this device by doing a dummy REQBUFS.
        // Obtain its capacities while we are at it.
        let capabilities: ioctl::BufferCapabilities =
            ioctl::reqbufs(&*device, queue_type, MemoryType::MMAP, 0)?;

        used_queues.insert(queue_type);

        drop(used_queues);

        Ok(Queue::<D, QueueInit> {
            inner: QueueBase {
                device,
                type_: queue_type,
                capabilities,
            },
            _d: std::marker::PhantomData,
            state: QueueInit {},
        })
    }

    /// Allocate `count` buffers for this queue and make it transition to the
    /// `BuffersAllocated` state.
    pub fn request_buffers<M: Memory>(self, count: u32) -> Result<Queue<D, BuffersAllocated<M>>> {
        let type_ = self.inner.type_;
        let num_buffers: usize =
            ioctl::reqbufs(&self.inner, type_, M::HandleType::MEMORY_TYPE, count)?;

        // The buffers have been allocated, now let's get their features.
        // We cannot use functional programming here because we need to return
        // the error from ioctl::querybuf(), if any.
        let mut buffer_features = Vec::new();
        for i in 0..num_buffers {
            buffer_features.push(ioctl::querybuf(&self.inner, self.inner.type_, i)?);
        }

        let buffer_info = buffer_features
            .into_iter()
            .map(|features: QueryBuffer| BufferInfo {
                state: Arc::new(Mutex::new(BufferState::Free)),
                features: Arc::new(features),
            })
            .collect();

        Ok(Queue {
            inner: self.inner,
            _d: std::marker::PhantomData,
            state: BuffersAllocated {
                num_queued_buffers: Default::default(),
                allocator: Arc::new(FifoBufferAllocator::new(num_buffers)),
                buffer_info,
            },
        })
    }
}

impl Queue<Output, QueueInit> {
    /// Acquires the OUTPUT queue from `device`.
    ///
    /// This method will fail if the queue has already been obtained and has not
    /// yet been released.
    pub fn get_output_queue(device: Arc<Device>) -> Result<Self> {
        Queue::<Output, QueueInit>::create(device, QueueType::VideoOutput)
    }

    /// Acquires the OUTPUT_MPLANE queue from `device`.
    ///
    /// This method will fail if the queue has already been obtained and has not
    /// yet been released.
    pub fn get_output_mplane_queue(device: Arc<Device>) -> Result<Self> {
        Queue::<Output, QueueInit>::create(device, QueueType::VideoOutputMplane)
    }
}

impl Queue<Capture, QueueInit> {
    /// Acquires the CAPTURE queue from `device`.
    ///
    /// This method will fail if the queue has already been obtained and has not
    /// yet been released.
    pub fn get_capture_queue(device: Arc<Device>) -> Result<Self> {
        Queue::<Capture, QueueInit>::create(device, QueueType::VideoCapture)
    }

    /// Acquires the CAPTURE_MPLANE queue from `device`.
    ///
    /// This method will fail if the queue has already been obtained and has not
    /// yet been released.
    pub fn get_capture_mplane_queue(device: Arc<Device>) -> Result<Self> {
        Queue::<Capture, QueueInit>::create(device, QueueType::VideoCaptureMplane)
    }
}

/// Represents a queued buffer which has not been processed due to `streamoff` being
/// called on the queue.
pub struct CanceledBuffer<M: Memory> {
    /// Index of the buffer,
    pub index: u32,
    /// Plane handles that were passed when the buffer has been queued.
    pub plane_handles: PlaneHandles<M>,
}

impl<D: Direction, M: Memory> Queue<D, BuffersAllocated<M>> {
    /// Returns the total number of buffers allocated for this queue.
    pub fn num_buffers(&self) -> usize {
        self.state.buffer_info.len()
    }

    /// Returns the number of buffers currently queued (i.e. being processed
    /// by the device).
    pub fn num_queued_buffers(&self) -> usize {
        self.state.num_queued_buffers.get()
    }

    pub fn streamon(&self) -> Result<()> {
        let type_ = self.inner.type_;
        ioctl::streamon(&self.inner, type_)
    }

    /// Stop streaming on this queue.
    ///
    /// If successful, then all the buffers that are queued but have not been
    /// dequeued yet return to the `Free` state. Buffer references obtained via
    /// `dequeue()` remain valid.
    pub fn streamoff(&self) -> Result<Vec<CanceledBuffer<M>>> {
        let type_ = self.inner.type_;
        ioctl::streamoff(&self.inner, type_)?;

        let canceled_buffers: Vec<_> = self
            .state
            .buffer_info
            .iter()
            .filter_map(|buffer_info| {
                let buffer_index = buffer_info.features.index;
                let mut state = buffer_info.state.lock().unwrap();
                // Filter entries not in queued state.
                match *state {
                    BufferState::Queued(_) => (),
                    _ => return None,
                };

                // Set entry to Free state and steal its handles.
                let old_state = std::mem::replace(&mut (*state), BufferState::Free);
                self.state.allocator.return_buffer(buffer_index);

                Some(CanceledBuffer::<M> {
                    index: buffer_index as u32,
                    plane_handles: match old_state {
                        // We have already tested for this state above, so this
                        // branch is guaranteed.
                        BufferState::Queued(plane_handles) => plane_handles,
                        _ => unreachable!("Inconsistent buffer state!"),
                    },
                })
            })
            .collect();

        let num_queued_buffers = self.state.num_queued_buffers.take();
        self.state
            .num_queued_buffers
            .set(num_queued_buffers - canceled_buffers.len());

        Ok(canceled_buffers)
    }

    // Take buffer `id` in order to prepare it for queueing, provided it is available.
    pub fn get_buffer<'a>(&'a self, index: usize) -> Result<QBuffer<'a, D, M>> {
        let buffer_info = self
            .state
            .buffer_info
            .get(index)
            .ok_or(Error::InvalidBuffer)?;

        let mut buffer_state = buffer_info.state.lock().unwrap();

        match *buffer_state {
            BufferState::Free => (),
            _ => return Err(Error::AlreadyBorrowed),
        };

        // The buffer will remain in PreQueue state until it is queued
        // or the reference to it is lost.
        *buffer_state = BufferState::PreQueue;

        self.state.allocator.take_buffer(index);
        drop(buffer_state);

        let fuse = BufferStateFuse::new(
            Arc::downgrade(&buffer_info.state),
            Arc::clone(&self.state.allocator),
            index,
        );

        Ok(QBuffer::new(self, &buffer_info.features, fuse))
    }

    pub fn get_free_buffer(&self) -> Option<QBuffer<D, M>> {
        let index = match self.state.allocator.get_free_buffer() {
            Some(index) => index,
            None => return None,
        };

        self.get_buffer(index).ok()
    }

    /// Dequeue the next processed buffer and return it.
    ///
    /// The V4L2 buffer can not be reused until the returned `DQBuffer` is
    /// dropped, so make sure to keep it around for as long as you need it. It can
    /// be moved into a `Rc` or `Arc` if you need to pass it to several clients.
    ///
    /// The data in the `DQBuffer` is read-only.
    pub fn dequeue(&self) -> Result<DQBuffer<D, M>> {
        let dqbuf: ioctl::DQBuffer = ioctl::dqbuf(&self.inner, self.inner.type_)?;
        let id = dqbuf.index as usize;

        let buffer_info = self
            .state
            .buffer_info
            .get(id)
            .expect("Inconsistent buffer state!");
        let mut buffer_state = buffer_info.state.lock().unwrap();

        // The buffer will remain Dequeued until our reference to it is destroyed.
        let state = std::mem::replace(&mut (*buffer_state), BufferState::Dequeued);
        let plane_handles = match state {
            BufferState::Queued(plane_handles) => plane_handles,
            _ => unreachable!("Inconsistent buffer state"),
        };
        let fuse = BufferStateFuse::new(
            Arc::downgrade(&buffer_info.state),
            Arc::clone(&self.state.allocator),
            id,
        );

        let num_queued_buffers = self.state.num_queued_buffers.take();
        self.state.num_queued_buffers.set(num_queued_buffers - 1);

        Ok(DQBuffer::new(
            self,
            &buffer_info.features,
            plane_handles,
            dqbuf,
            fuse,
        ))
    }

    /// Release all the allocated buffers and returns the queue to the `Init` state.
    pub fn free_buffers(self) -> Result<Queue<D, QueueInit>> {
        let type_ = self.inner.type_;
        ioctl::reqbufs(&self.inner, type_, M::HandleType::MEMORY_TYPE, 0)?;

        Ok(Queue {
            inner: self.inner,
            _d: std::marker::PhantomData,
            state: QueueInit {},
        })
    }
}

/// A fuse that will return the buffer to the Free state when destroyed, unless
/// it has been disarmed.
struct BufferStateFuse<M: Memory> {
    buffer_state: Weak<Mutex<BufferState<M>>>,
    allocator: Arc<dyn BufferAllocator>,
    index: usize,
}

impl<M: Memory> BufferStateFuse<M> {
    /// Create a new fuse that will set `state` to `BufferState::Free` if
    /// destroyed before `disarm()` has been called.
    fn new(
        buffer_state: Weak<Mutex<BufferState<M>>>,
        allocator: Arc<dyn BufferAllocator>,
        index: usize,
    ) -> Self {
        BufferStateFuse {
            buffer_state,
            allocator,
            index,
        }
    }

    /// Disarm this fuse, e.g. the monitored state will be left untouched when
    /// the fuse is destroyed.
    fn disarm(&mut self) {
        // Drop our weak reference.
        self.buffer_state = Weak::new();
    }

    /// Trigger the fuse, i.e. make the buffer return to the Free state, unless
    /// the fuse has been `disarm`ed. This method should only be called when
    /// the buffer is being dropped, otherwise inconsistent state may ensue.
    /// The fuse will be disarmed after this call.
    fn trigger(&mut self) {
        match self.buffer_state.upgrade() {
            None => (),
            Some(buffer_state_unlocked) => {
                let mut buffer_state = buffer_state_unlocked.lock().unwrap();
                *buffer_state = BufferState::Free;
                self.allocator.return_buffer(self.index);
                self.buffer_state = Weak::new();
            }
        };
    }
}

impl<M: Memory> Drop for BufferStateFuse<M> {
    fn drop(&mut self) {
        self.trigger();
    }
}

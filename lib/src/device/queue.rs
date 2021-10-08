pub mod direction;
pub mod dqbuf;
pub mod generic;
pub mod handles_provider;
pub mod qbuf;
pub mod states;

use self::qbuf::{get_free::GetFreeOutputBuffer, get_indexed::GetOutputBufferByIndex};

use super::{AllocatedQueue, Device, FreeBuffersResult, Stream, TryDequeue};
use crate::{
    ioctl::{
        self, DqBufError, DqBufResult, Fmt, GFmtError, QueryBuffer, SFmtError, SelectionTarget,
        SelectionType, StreamOffError, StreamOnError, TryFmtError,
    },
    PlaneLayout, Rect,
};
use crate::{memory::*, FormatConversionError};
use crate::{Format, PixelFormat, QueueType};
use direction::*;
use dqbuf::*;
use generic::{GenericBufferHandles, GenericQBuffer, GenericSupportedMemoryType};
use log::debug;
use qbuf::{
    get_free::{GetFreeBufferError, GetFreeCaptureBuffer},
    get_indexed::{GetCaptureBufferByIndex, TryGetBufferError},
    *,
};
use states::*;

use std::os::unix::io::{AsRawFd, RawFd};
use std::{
    cell::Cell,
    sync::{Arc, Mutex, Weak},
};
use thiserror::Error;

/// Base values of a queue, that are always value no matter the state the queue
/// is in. This base object remains alive as long as the queue is borrowed from
/// the `Device`.
pub struct QueueBase {
    // Reference to the device, so we can perform operations on its `fd` and to let us mark the
    // queue as free again upon destruction.
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
        assert!(self.device.used_queues.lock().unwrap().remove(&self.type_));
    }
}

/// Trait for the different states a queue can be in. This allows us to limit
/// the available queue methods to the one that make sense at a given point of
/// the queue's lifecycle.
pub trait QueueState {}

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

    pub fn get_format<E: Into<FormatConversionError>, T: Fmt<E>>(&self) -> Result<T, GFmtError> {
        ioctl::g_fmt(&self.inner, self.inner.type_)
    }

    /// This method can invalidate any current format iterator, hence it requires
    /// the queue to be mutable. This way of doing is not perfect though, as setting
    /// the format on one queue can change the options available on another.
    pub fn set_format(&mut self, format: Format) -> Result<Format, SFmtError> {
        let type_ = self.inner.type_;
        ioctl::s_fmt(&mut self.inner, type_, format)
    }

    /// Performs exactly as `set_format`, but does not actually apply `format`.
    /// Useful to check what modifications need to be done to a format before it
    /// can be used.
    pub fn try_format(&self, format: Format) -> Result<Format, TryFmtError> {
        ioctl::try_fmt(&self.inner, self.inner.type_, format)
    }

    /// Returns a `FormatBuilder` which is set to the currently active format
    /// and can be modified and eventually applied. The `FormatBuilder` holds
    /// a mutable reference to this `Queue`.
    pub fn change_format(&mut self) -> Result<FormatBuilder, GFmtError> {
        FormatBuilder::new(&mut self.inner)
    }

    /// Returns an iterator over all the formats currently supported by this queue.
    pub fn format_iter(&self) -> ioctl::FormatIterator<QueueBase> {
        ioctl::FormatIterator::new(&self.inner, self.inner.type_)
    }

    pub fn get_selection(&self, target: SelectionTarget) -> Result<Rect, ioctl::GSelectionError> {
        let selection = match self.get_type() {
            QueueType::VideoCapture | QueueType::VideoCaptureMplane => SelectionType::Capture,
            QueueType::VideoOutput | QueueType::VideoOutputMplane => SelectionType::Output,
        };

        ioctl::g_selection(&self.inner, selection, target)
    }
}

/// Builder for a V4L2 format. This takes a mutable reference on the queue, so
/// it is supposed to be short-lived: get one, adjust the format, and apply.
pub struct FormatBuilder<'a> {
    queue: &'a mut QueueBase,
    format: Format,
}

impl<'a> FormatBuilder<'a> {
    fn new(queue: &'a mut QueueBase) -> Result<Self, GFmtError> {
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

    pub fn set_planes_layout<P: IntoIterator<Item = PlaneLayout>>(mut self, planes: P) -> Self {
        self.format.plane_fmt = planes.into_iter().collect();
        self
    }

    /// Apply the format built so far. The kernel will adjust the format to fit
    /// the driver's capabilities if needed, and the format actually applied will
    /// be returned.
    pub fn apply<E: Into<FormatConversionError>, T: ioctl::Fmt<E>>(self) -> Result<T, SFmtError> {
        ioctl::s_fmt(self.queue, self.queue.type_, self.format)
    }

    /// Try to apply the format built so far. The kernel will adjust the format
    /// to fit the driver's capabilities if needed, so make sure to check important
    /// parameters upon return.
    ///
    /// Calling `apply()` right after this method is guaranteed to successfully
    /// apply the format without further change.
    pub fn try_apply(&mut self) -> Result<(), TryFmtError> {
        let new_format = ioctl::try_fmt(self.queue, self.queue.type_, self.format.clone())?;

        self.format = new_format;
        Ok(())
    }
}

/// Initial state of the queue when created. Streaming and queuing are not
/// supported since buffers have not been allocated yet.
/// Allocating buffers makes the queue switch to the `BuffersAllocated` state.
pub struct QueueInit;
impl QueueState for QueueInit {}

#[derive(Debug, Error)]
pub enum CreateQueueError {
    #[error("Queue is already in use")]
    AlreadyBorrowed,
    #[error("Error while querying queue capabilities")]
    ReqbufsError(#[from] ioctl::ReqbufsError),
}

#[derive(Debug, Error)]
pub enum RequestBuffersError {
    #[error("Error while requesting buffers")]
    ReqbufsError(#[from] ioctl::ReqbufsError),
    #[error("Error while querying buffer")]
    QueryBufferError(#[from] ioctl::QueryBufError),
}

impl<D: Direction> Queue<D, QueueInit> {
    /// Create a queue for type `queue_type` on `device`. A queue of a specific type
    /// can be requested only once.
    ///
    /// Not all devices support all kinds of queue. To test whether the queue is supported,
    /// a REQBUFS(0) is issued on the device. If it is not successful, the device is
    /// deemed to not support this kind of queue and this method will fail.
    fn create(
        device: Arc<Device>,
        queue_type: QueueType,
    ) -> Result<Queue<D, QueueInit>, CreateQueueError> {
        let mut used_queues = device.used_queues.lock().unwrap();

        if used_queues.contains(&queue_type) {
            return Err(CreateQueueError::AlreadyBorrowed);
        }

        // Check that the queue is valid for this device by doing a dummy REQBUFS.
        // Obtain its capacities while we are at it.
        let capabilities: ioctl::BufferCapabilities =
            ioctl::reqbufs(&*device, queue_type, MemoryType::Mmap, 0)?;

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

    pub fn request_buffers_generic<P: BufferHandles>(
        self,
        memory_type: P::SupportedMemoryType,
        count: u32,
    ) -> Result<Queue<D, BuffersAllocated<P>>, RequestBuffersError> {
        let type_ = self.inner.type_;
        let num_buffers: usize = ioctl::reqbufs(&self.inner, type_, memory_type.into(), count)?;

        debug!(
            "Requested {} buffers on {} queue, obtained {}",
            count, type_, num_buffers
        );

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
                memory_type,
                num_queued_buffers: Default::default(),
                buffer_info,
            },
        })
    }

    /// Allocate `count` buffers for this queue and make it transition to the
    /// `BuffersAllocated` state.
    pub fn request_buffers<P: PrimitiveBufferHandles>(
        self,
        count: u32,
    ) -> Result<Queue<D, BuffersAllocated<P>>, RequestBuffersError> {
        self.request_buffers_generic(P::MEMORY_TYPE, count)
    }
}

impl Queue<Output, QueueInit> {
    /// Acquires the OUTPUT queue from `device`.
    ///
    /// This method will fail if the queue has already been obtained and has not
    /// yet been released.
    pub fn get_output_queue(device: Arc<Device>) -> Result<Self, CreateQueueError> {
        Queue::<Output, QueueInit>::create(device, QueueType::VideoOutput)
    }

    /// Acquires the OUTPUT_MPLANE queue from `device`.
    ///
    /// This method will fail if the queue has already been obtained and has not
    /// yet been released.
    pub fn get_output_mplane_queue(device: Arc<Device>) -> Result<Self, CreateQueueError> {
        Queue::<Output, QueueInit>::create(device, QueueType::VideoOutputMplane)
    }
}

impl Queue<Capture, QueueInit> {
    /// Acquires the CAPTURE queue from `device`.
    ///
    /// This method will fail if the queue has already been obtained and has not
    /// yet been released.
    pub fn get_capture_queue(device: Arc<Device>) -> Result<Self, CreateQueueError> {
        Queue::<Capture, QueueInit>::create(device, QueueType::VideoCapture)
    }

    /// Acquires the CAPTURE_MPLANE queue from `device`.
    ///
    /// This method will fail if the queue has already been obtained and has not
    /// yet been released.
    pub fn get_capture_mplane_queue(device: Arc<Device>) -> Result<Self, CreateQueueError> {
        Queue::<Capture, QueueInit>::create(device, QueueType::VideoCaptureMplane)
    }
}

/// Allocated state for a queue. A queue with its buffers allocated can be
/// streamed on and off, and buffers can be queued and dequeued.
pub struct BuffersAllocated<P: BufferHandles> {
    memory_type: P::SupportedMemoryType,
    num_queued_buffers: Cell<usize>,
    buffer_info: Vec<BufferInfo<P>>,
}
impl<P: BufferHandles> QueueState for BuffersAllocated<P> {}

impl<D: Direction, P: BufferHandles> Queue<D, BuffersAllocated<P>> {
    /// Return all the currently queued buffers as CanceledBuffers. This can
    /// be called after a explicit or implicit streamoff to inform the client
    /// of which buffers have been canceled and return their handles.
    fn cancel_queued_buffers(&self) -> Vec<CanceledBuffer<P>> {
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

                Some(CanceledBuffer {
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

        debug!(
            "{} buffers canceled on {} queue",
            canceled_buffers.len(),
            self.get_type()
        );

        let num_queued_buffers = self.state.num_queued_buffers.take();
        assert_eq!(num_queued_buffers, canceled_buffers.len());
        self.state.num_queued_buffers.set(0);

        canceled_buffers
    }

    fn try_get_buffer_info(&self, index: usize) -> Result<&BufferInfo<P>, TryGetBufferError> {
        let buffer_info = self
            .state
            .buffer_info
            .get(index)
            .ok_or(TryGetBufferError::InvalidIndex(index))?;

        let mut buffer_state = buffer_info.state.lock().unwrap();
        match *buffer_state {
            BufferState::Free => (),
            _ => return Err(TryGetBufferError::AlreadyUsed),
        };

        // The buffer will remain in PreQueue state until it is queued
        // or the reference to it is lost.
        *buffer_state = BufferState::PreQueue;
        drop(buffer_state);

        Ok(buffer_info)
    }
}

impl<'a, D: Direction, P: BufferHandles + 'a> AllocatedQueue<'a, D>
    for Queue<D, BuffersAllocated<P>>
{
    fn num_buffers(&self) -> usize {
        self.state.buffer_info.len()
    }

    fn num_queued_buffers(&self) -> usize {
        self.state.num_queued_buffers.get()
    }

    fn free_buffers(self) -> Result<FreeBuffersResult<D, Self>, ioctl::ReqbufsError> {
        let type_ = self.inner.type_;
        ioctl::reqbufs(&self.inner, type_, self.state.memory_type.into(), 0)?;

        debug!("Freed all buffers on {} queue", type_);

        // reqbufs also performs an implicit streamoff, so return the cancelled
        // buffers.
        let canceled_buffers = self.cancel_queued_buffers();

        Ok(FreeBuffersResult {
            queue: Queue {
                inner: self.inner,
                _d: std::marker::PhantomData,
                state: QueueInit {},
            },
            canceled_buffers,
        })
    }
}

/// Represents a queued buffer which has not been processed due to `streamoff`
/// being called on a queue.
pub struct CanceledBuffer<P: BufferHandles> {
    /// Index of the buffer,
    pub index: u32,
    /// Plane handles that were passed when the buffer has been queued.
    pub plane_handles: P,
}

impl<D: Direction, P: BufferHandles> Stream for Queue<D, BuffersAllocated<P>> {
    type Canceled = CanceledBuffer<P>;

    fn stream_on(&self) -> Result<(), StreamOnError> {
        debug!("{} queue streaming on", self.get_type());
        let type_ = self.inner.type_;
        ioctl::streamon(&self.inner, type_)
    }

    fn stream_off(&self) -> Result<Vec<Self::Canceled>, StreamOffError> {
        debug!("{} queue streaming off", self.get_type());
        let type_ = self.inner.type_;
        ioctl::streamoff(&self.inner, type_)?;

        Ok(self.cancel_queued_buffers())
    }
}

impl<D: Direction, P: BufferHandles> TryDequeue for Queue<D, BuffersAllocated<P>> {
    type Dequeued = DqBuffer<D, P>;

    fn try_dequeue(&self) -> DqBufResult<Self::Dequeued> {
        let dqbuf: ioctl::DqBuffer;
        let mut error_flag_set = false;

        dqbuf = match ioctl::dqbuf(&self.inner, self.inner.type_) {
            Ok(dqbuf) => dqbuf,
            Err(DqBufError::CorruptedBuffer(dqbuf)) => {
                error_flag_set = true;
                dqbuf
            }
            Err(DqBufError::Eos) => return Err(DqBufError::Eos),
            Err(DqBufError::NotReady) => return Err(DqBufError::NotReady),
            Err(DqBufError::IoctlError(e)) => return Err(DqBufError::IoctlError(e)),
        };

        let id = dqbuf.index() as usize;

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
        let fuse = BufferStateFuse::new(Arc::downgrade(&buffer_info.state));

        let num_queued_buffers = self.state.num_queued_buffers.take();
        self.state.num_queued_buffers.set(num_queued_buffers - 1);

        let dqbuffer = DqBuffer::new(self, &buffer_info.features, plane_handles, dqbuf, fuse);

        if error_flag_set {
            Err(DqBufError::CorruptedBuffer(dqbuffer))
        } else {
            Ok(dqbuffer)
        }
    }
}

mod private {
    use super::*;

    /// Private trait for providing a Queuable regardless of the queue's
    /// direction. Avoids duplicating the same code in
    /// Capture/OutputQueueableProvider's implementations.
    pub trait GetBufferByIndex<'a> {
        type Queueable: 'a;

        fn try_get_buffer(&'a self, index: usize) -> Result<Self::Queueable, TryGetBufferError>;
    }

    /// Same as `GetBufferByIndex` but for providing any free buffer.
    pub trait GetFreeBuffer<'a, ErrorType = GetFreeBufferError>: GetBufferByIndex<'a> {
        fn try_get_free_buffer(&'a self) -> Result<Self::Queueable, ErrorType>;
    }

    impl<'a, D: Direction, P: PrimitiveBufferHandles> GetBufferByIndex<'a>
        for Queue<D, BuffersAllocated<P>>
    {
        type Queueable = QBuffer<'a, D, P, P>;

        // Take buffer `id` in order to prepare it for queueing, provided it is available.
        fn try_get_buffer(&'a self, index: usize) -> Result<Self::Queueable, TryGetBufferError> {
            Ok(QBuffer::new(self, self.try_get_buffer_info(index)?))
        }
    }

    impl<'a, D: Direction> GetBufferByIndex<'a> for Queue<D, BuffersAllocated<GenericBufferHandles>> {
        type Queueable = GenericQBuffer<'a, D>;

        fn try_get_buffer(&'a self, index: usize) -> Result<Self::Queueable, TryGetBufferError> {
            let buffer_info = self.try_get_buffer_info(index)?;

            Ok(match self.state.memory_type {
                GenericSupportedMemoryType::Mmap => {
                    GenericQBuffer::Mmap(QBuffer::new(self, buffer_info))
                }
                GenericSupportedMemoryType::UserPtr => {
                    GenericQBuffer::User(QBuffer::new(self, buffer_info))
                }
                GenericSupportedMemoryType::DmaBuf => {
                    GenericQBuffer::DmaBuf(QBuffer::new(self, buffer_info))
                }
            })
        }
    }

    impl<'a, D, P> GetFreeBuffer<'a> for Queue<D, BuffersAllocated<P>>
    where
        D: Direction,
        P: BufferHandles,
        Self: GetBufferByIndex<'a>,
    {
        fn try_get_free_buffer(&'a self) -> Result<Self::Queueable, GetFreeBufferError> {
            let res = self
                .state
                .buffer_info
                .iter()
                .enumerate()
                .find(|(_, s)| matches!(*s.state.lock().unwrap(), BufferState::Free));

            match res {
                None => Err(GetFreeBufferError::NoFreeBuffer),
                Some((i, _)) => Ok(self.try_get_buffer(i).unwrap()),
            }
        }
    }
}

impl<'a, P: PrimitiveBufferHandles> CaptureQueueableProvider<'a, P>
    for Queue<Capture, BuffersAllocated<P>>
where
    Self: private::GetBufferByIndex<'a>,
    <Self as private::GetBufferByIndex<'a>>::Queueable: CaptureQueueable<P>,
{
    type Queueable = <Self as private::GetBufferByIndex<'a>>::Queueable;
}

impl<'a, P: PrimitiveBufferHandles> OutputQueueableProvider<'a, P>
    for Queue<Output, BuffersAllocated<P>>
where
    Self: private::GetBufferByIndex<'a>,
    <Self as private::GetBufferByIndex<'a>>::Queueable: OutputQueueable<P>,
{
    type Queueable = <Self as private::GetBufferByIndex<'a>>::Queueable;
}

impl<'a, P: BufferHandles, R> GetOutputBufferByIndex<'a, P> for Queue<Output, BuffersAllocated<P>>
where
    Self: private::GetBufferByIndex<'a, Queueable = R>,
    Self: OutputQueueableProvider<'a, P, Queueable = R>,
{
    // Take buffer `id` in order to prepare it for queueing, provided it is available.
    fn try_get_buffer(&'a self, index: usize) -> Result<Self::Queueable, TryGetBufferError> {
        <Self as private::GetBufferByIndex<'a>>::try_get_buffer(self, index)
    }
}

impl<'a, P: BufferHandles, R> GetCaptureBufferByIndex<'a, P> for Queue<Capture, BuffersAllocated<P>>
where
    Self: private::GetBufferByIndex<'a, Queueable = R>,
    Self: CaptureQueueableProvider<'a, P, Queueable = R>,
{
    // Take buffer `id` in order to prepare it for queueing, provided it is available.
    fn try_get_buffer(&'a self, index: usize) -> Result<Self::Queueable, TryGetBufferError> {
        <Self as private::GetBufferByIndex<'a>>::try_get_buffer(self, index)
    }
}

impl<'a, P: BufferHandles, R> GetFreeOutputBuffer<'a, P> for Queue<Output, BuffersAllocated<P>>
where
    Self: private::GetFreeBuffer<'a, Queueable = R>,
    Self: OutputQueueableProvider<'a, P, Queueable = R>,
{
    fn try_get_free_buffer(&'a self) -> Result<Self::Queueable, GetFreeBufferError> {
        <Self as private::GetFreeBuffer<'a>>::try_get_free_buffer(self)
    }
}

impl<'a, P: BufferHandles, R> GetFreeCaptureBuffer<'a, P> for Queue<Capture, BuffersAllocated<P>>
where
    Self: private::GetFreeBuffer<'a, Queueable = R>,
    Self: CaptureQueueableProvider<'a, P, Queueable = R>,
{
    fn try_get_free_buffer(&'a self) -> Result<Self::Queueable, GetFreeBufferError> {
        <Self as private::GetFreeBuffer<'a>>::try_get_free_buffer(self)
    }
}

/// A fuse that will return the buffer to the Free state when destroyed, unless
/// it has been disarmed.
struct BufferStateFuse<P: BufferHandles> {
    buffer_state: Weak<Mutex<BufferState<P>>>,
}

impl<P: BufferHandles> BufferStateFuse<P> {
    /// Create a new fuse that will set `state` to `BufferState::Free` if
    /// destroyed before `disarm()` has been called.
    fn new(buffer_state: Weak<Mutex<BufferState<P>>>) -> Self {
        BufferStateFuse { buffer_state }
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
            Some(buffer_state_locked) => {
                let mut buffer_state = buffer_state_locked.lock().unwrap();
                *buffer_state = BufferState::Free;
                self.buffer_state = Weak::new();
            }
        };
    }
}

impl<P: BufferHandles> Drop for BufferStateFuse<P> {
    fn drop(&mut self) {
        self.trigger();
    }
}

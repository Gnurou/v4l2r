pub mod buffer;
pub mod direction;
pub mod dqbuf;
pub mod generic;
pub mod handles_provider;
pub mod qbuf;

use super::{AllocatedQueue, Device, FreeBuffersResult, Stream, TryDequeue};
use crate::ioctl::{DqBufResult, QueryBufError, V4l2BufferFromError};
use crate::{bindings, memory::*};
use crate::{
    ioctl::{
        self, GFmtError, QueryBuffer, ReqbufsError, SFmtError, SelectionTarget, SelectionType,
        StreamOffError, StreamOnError, TryFmtError,
    },
    PlaneLayout, Rect,
};
use crate::{Format, PixelFormat, QueueType};
use buffer::*;
use direction::*;
use dqbuf::*;
use generic::{GenericBufferHandles, GenericQBuffer, GenericSupportedMemoryType};
use log::debug;
use qbuf::*;

use std::convert::{Infallible, TryFrom};
use std::os::unix::io::{AsRawFd, RawFd};
use std::sync::{Arc, Weak};
use thiserror::Error;

/// Base values of a queue, that are always value no matter the state the queue
/// is in. This base object remains alive as long as the queue is borrowed from
/// the `Device`.
struct QueueBase {
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

impl Drop for QueueBase {
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

    pub fn get_format<T: TryFrom<bindings::v4l2_format>>(&self) -> Result<T, GFmtError> {
        ioctl::g_fmt(&self.inner, self.inner.type_)
    }

    /// This method can invalidate any current format iterator, hence it requires
    /// the queue to be mutable. This way of doing is not perfect though, as setting
    /// the format on one queue can change the options available on another.
    pub fn set_format(&mut self, format: Format) -> Result<Format, SFmtError> {
        let type_ = self.inner.type_;
        ioctl::s_fmt(&mut self.inner, (type_, &format))
    }

    /// Performs exactly as `set_format`, but does not actually apply `format`.
    /// Useful to check what modifications need to be done to a format before it
    /// can be used.
    pub fn try_format(&self, format: Format) -> Result<Format, TryFmtError> {
        ioctl::try_fmt(&self.inner, (self.inner.type_, &format))
    }

    /// Returns a `FormatBuilder` which is set to the currently active format
    /// and can be modified and eventually applied. The `FormatBuilder` holds
    /// a mutable reference to this `Queue`.
    pub fn change_format(&mut self) -> Result<FormatBuilder, GFmtError> {
        FormatBuilder::new(&mut self.inner)
    }

    /// Returns an iterator over all the formats currently supported by this queue.
    pub fn format_iter(&self) -> ioctl::FormatIterator<Device> {
        ioctl::FormatIterator::new(self.inner.device.as_ref(), self.inner.type_)
    }

    pub fn get_selection(&self, target: SelectionTarget) -> Result<Rect, ioctl::GSelectionError> {
        let selection = match self.get_type() {
            QueueType::VideoCapture | QueueType::VideoCaptureMplane => SelectionType::Capture,
            QueueType::VideoOutput | QueueType::VideoOutputMplane => SelectionType::Output,
            _ => return Err(ioctl::GSelectionError::Invalid),
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
    pub fn apply<O: TryFrom<bindings::v4l2_format>>(self) -> Result<O, SFmtError> {
        ioctl::s_fmt(self.queue, (self.queue.type_, &self.format))
    }

    /// Try to apply the format built so far. The kernel will adjust the format
    /// to fit the driver's capabilities if needed, so make sure to check important
    /// parameters upon return.
    ///
    /// Calling `apply()` right after this method is guaranteed to successfully
    /// apply the format without further change.
    pub fn try_apply(&mut self) -> Result<(), TryFmtError> {
        let new_format = ioctl::try_fmt(self.queue, (self.queue.type_, &self.format))?;

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
    #[error("queue is already in use")]
    AlreadyBorrowed,
    #[error("error while querying queue capabilities")]
    ReqbufsError(#[from] ioctl::ReqbufsError),
}

#[derive(Debug, Error)]
pub enum RequestBuffersError {
    #[error("error while requesting buffers")]
    ReqbufsError(#[from] ioctl::ReqbufsError),
    #[error("error while querying buffer")]
    QueryBufferError(#[from] QueryBufError<Infallible>),
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
            ioctl::reqbufs(&*device, queue_type, MemoryType::Mmap, 0)
                // In the unlikely case that MMAP buffers are not supported, try DMABUF.
                .or_else(|e| match e {
                    ReqbufsError::InvalidBufferType(_, _) => {
                        ioctl::reqbufs(&*device, queue_type, MemoryType::DmaBuf, 0)
                    }
                    _ => Err(e),
                })?;

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

        let buffer_stats = Arc::new(BufferStats::new());

        let buffer_info = buffer_features
            .into_iter()
            .map(|features: QueryBuffer| {
                Arc::new(BufferInfo::new(features, Arc::clone(&buffer_stats)))
            })
            .collect();

        Ok(Queue {
            inner: self.inner,
            _d: std::marker::PhantomData,
            state: BuffersAllocated {
                memory_type,
                buffer_info,
                buffer_stats,
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
    /// Keep one `Arc` per buffer. This allows us to invalidate this buffer only in case it gets
    /// deallocated alone (V4L2 currently does not allow this, but might in the future).
    buffer_info: Vec<Arc<BufferInfo<P>>>,
    buffer_stats: Arc<BufferStats>,
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
                // Take the handles of queued entries and make them free again.
                // Skip entries in any other state.
                let plane_handles = buffer_info.update_state(|state| {
                    match *state {
                        // Set queued entry to `Free` state and steal its handles.
                        BufferState::Queued(_) => {
                            // We just matched the state but need to do it again in order to take
                            // the handles since `state` is a reference...
                            match std::mem::replace(state, BufferState::Free) {
                                BufferState::Queued(handles) => Some(handles),
                                _ => unreachable!(),
                            }
                        }
                        // Filter out entries not in queued state.
                        _ => None,
                    }
                })?;

                Some(CanceledBuffer {
                    index: buffer_info.features.index as u32,
                    plane_handles,
                })
            })
            .collect();

        debug!(
            "{} buffers canceled on {} queue",
            canceled_buffers.len(),
            self.get_type()
        );

        assert_eq!(self.state.buffer_stats.num_queued(), 0);

        canceled_buffers
    }

    /// Try to obtain a buffer to pass to userspace so it can be queued. `index` must be the index
    /// of a buffer in the `Free` state, otherwise an `AlreadyUsed` error is returned.
    fn try_obtain_buffer(&self, index: usize) -> Result<&Arc<BufferInfo<P>>, TryGetBufferError> {
        let buffer_info = self
            .state
            .buffer_info
            .get(index)
            .ok_or(TryGetBufferError::InvalidIndex(index))?;

        buffer_info.update_state(|state| match *state {
            BufferState::Free => {
                *state = BufferState::PreQueue;
                Ok(())
            }
            _ => Err(TryGetBufferError::AlreadyUsed),
        })?;

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
        self.state.buffer_stats.num_queued()
    }

    fn num_free_buffers(&self) -> usize {
        self.state.buffer_stats.num_free()
    }

    fn free_buffers(self) -> Result<FreeBuffersResult<D, Self>, ioctl::ReqbufsError> {
        let type_ = self.inner.type_;
        ioctl::reqbufs::<()>(&self.inner, type_, self.state.memory_type.into(), 0)?;

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

    fn try_dequeue(&self) -> DqBufResult<Self::Dequeued, V4l2BufferFromError> {
        let dqbuf: ioctl::V4l2Buffer = ioctl::dqbuf(&self.inner, self.inner.type_)?;

        let id = dqbuf.index() as usize;

        let buffer_info = self
            .state
            .buffer_info
            .get(id)
            .expect("Inconsistent buffer state!");

        let plane_handles = buffer_info.update_state(|state| match *state {
            BufferState::Queued(_) => {
                // We just matched the state but need to do it again in order to take the handles
                // since `state` is a reference...
                match std::mem::replace(state, BufferState::Dequeued) {
                    BufferState::Queued(handles) => handles,
                    _ => unreachable!(),
                }
            }
            _ => unreachable!("Inconsistent buffer state!"),
        });

        let fuse = BufferStateFuse::new(Arc::downgrade(buffer_info));

        let dqbuffer = DqBuffer::new(self, buffer_info, plane_handles, dqbuf, fuse);

        Ok(dqbuffer)
    }
}

#[derive(Debug, Error)]
pub enum TryGetBufferError {
    #[error("buffer with provided index {0} does not exist")]
    InvalidIndex(usize),
    #[error("buffer is already in use")]
    AlreadyUsed,
}

#[derive(Debug, Error)]
pub enum GetFreeBufferError {
    #[error("all buffers are currently being used")]
    NoFreeBuffer,
}

mod private {
    use std::ops::Deref;

    use super::*;

    /// Private trait for providing a Queuable regardless of the queue's direction.
    ///
    /// This avoids duplicating the same code in Capture/OutputQueueableProvider's implementations.
    ///
    /// The lifetime `'a` is here to allow implementations to attach the lifetime of their return
    /// value to `self`. This is useful when we want the buffer to hold a reference to the queue
    /// that prevents the latter from mutating as long as the buffer is not consumed.
    pub trait GetBufferByIndex<'a> {
        type Queueable;

        fn try_get_buffer(&'a self, index: usize) -> Result<Self::Queueable, TryGetBufferError>;
    }

    /// Same as `GetBufferByIndex` but for providing any free buffer.
    pub trait GetFreeBuffer<'a, ErrorType = GetFreeBufferError>: GetBufferByIndex<'a> {
        fn try_get_free_buffer(&'a self) -> Result<Self::Queueable, ErrorType>;
    }

    impl<'a, D: Direction, P: PrimitiveBufferHandles> GetBufferByIndex<'a>
        for Queue<D, BuffersAllocated<P>>
    {
        type Queueable = QBuffer<D, P, P, &'a Queue<D, BuffersAllocated<P>>>;

        // Take buffer `id` in order to prepare it for queueing, provided it is available.
        fn try_get_buffer(&'a self, index: usize) -> Result<Self::Queueable, TryGetBufferError> {
            Ok(QBuffer::new(self, self.try_obtain_buffer(index)?))
        }
    }

    impl<'a, D: Direction> GetBufferByIndex<'a> for Queue<D, BuffersAllocated<GenericBufferHandles>> {
        type Queueable = GenericQBuffer<D, &'a Self>;

        fn try_get_buffer(&'a self, index: usize) -> Result<Self::Queueable, TryGetBufferError> {
            let buffer_info = self.try_obtain_buffer(index)?;

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

    /// Allows to obtain a [`QBuffer`] with a `'static` lifetime from e.g. an `Arc<Queue>`.
    ///
    /// [`QBuffer`]s obtained directly from a [`Queue`] maintain consistency by holding a reference
    /// to the [`Queue`], which can be inconvenient if we need to keep the [`QBuffer`] aside for
    /// some time. This implementation allows [`QBuffer`]s to be created with a static lifetime
    /// from a queue behind a cloneable and dereferencable type (typically [`std::rc::Rc`] or
    /// [`std::sync::Arc`]).
    ///
    /// This added flexibility comes with the counterpart that the user must unwrap the [`Queue`]
    /// from its container reference before applying mutable operations to it like
    /// [`Queue::request_buffers`]. Doing so requires calling methods like
    /// [`std::sync::Arc::into_inner`], which only succeed if there is no other reference to the
    /// queue, preserving consistency explicitly at runtime instead of implicitly at compile-time.
    impl<'a, D, P, Q> GetBufferByIndex<'a> for Q
    where
        D: Direction,
        P: PrimitiveBufferHandles,
        Q: Deref<Target = Queue<D, BuffersAllocated<P>>> + Clone,
    {
        type Queueable = QBuffer<D, P, P, Q>;

        fn try_get_buffer(&'a self, index: usize) -> Result<Self::Queueable, TryGetBufferError> {
            Ok(QBuffer::new(self.clone(), self.try_obtain_buffer(index)?))
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
                .find(|(_, s)| s.do_with_state(|s| matches!(s, BufferState::Free)));

            match res {
                None => Err(GetFreeBufferError::NoFreeBuffer),
                Some((i, _)) => Ok(self.try_get_buffer(i).unwrap()),
            }
        }
    }

    /// Allows to obtain a [`QBuffer`] with a `'static` lifetime from e.g. an `Arc<Queue>`.
    ///
    /// [`QBuffer`]s obtained directly from a [`Queue`] maintain consistency by holding a reference
    /// to the [`Queue`], which can be inconvenient if we need to keep the [`QBuffer`] aside for
    /// some time. This implementation allows [`QBuffer`]s to be created with a static lifetime
    /// from a queue behind a cloneable and dereferencable type (typically [`std::rc::Rc`] or
    /// [`std::sync::Arc`]).
    ///
    /// This added flexibility comes with the counterpart that the user must unwrap the [`Queue`]
    /// from its container reference before applying mutable operations to it like
    /// [`Queue::request_buffers`]. Doing so requires calling methods like
    /// [`std::sync::Arc::into_inner`], which only succeed if there is no other reference to the
    /// queue, preserving consistency explicitly at runtime instead of implicitly at compile-time.
    impl<'a, D, P, Q> GetFreeBuffer<'a> for Q
    where
        D: Direction,
        P: PrimitiveBufferHandles,
        Q: Deref<Target = Queue<D, BuffersAllocated<P>>> + Clone,
    {
        fn try_get_free_buffer(&'a self) -> Result<Self::Queueable, GetFreeBufferError> {
            let res = self
                .state
                .buffer_info
                .iter()
                .enumerate()
                .find(|(_, s)| s.do_with_state(|s| matches!(s, BufferState::Free)));

            match res {
                None => Err(GetFreeBufferError::NoFreeBuffer),
                Some((i, _)) => Ok(self.try_get_buffer(i).unwrap()),
            }
        }
    }
}

/// Trait for queueable CAPTURE buffers. These buffers only require handles to
/// be queued.
pub trait CaptureQueueable<B: BufferHandles> {
    /// Queue the buffer after binding `handles`, consuming the object.
    /// The number of handles must match the buffer's expected number of planes.
    fn queue_with_handles(self, handles: B) -> QueueResult<(), B>;
}

/// Trait for queueable OUTPUT buffers. The number of bytes used must be
/// specified for each plane.
pub trait OutputQueueable<B: BufferHandles> {
    /// Queue the buffer after binding `handles`, consuming the object.
    /// The number of handles must match the buffer's expected number of planes.
    /// `bytes_used` must be a slice with as many slices as there are handles,
    /// describing the amount of useful data in each of them.
    fn queue_with_handles(self, handles: B, bytes_used: &[usize]) -> QueueResult<(), B>;
}

/// Trait for all objects that are capable of providing objects that can be
/// queued to the CAPTURE queue.
pub trait CaptureQueueableProvider<'a, B: BufferHandles> {
    type Queueable: CaptureQueueable<B>;
}

impl<'a, B, Q> CaptureQueueableProvider<'a, B> for Q
where
    B: BufferHandles,
    Q: private::GetBufferByIndex<'a>,
    <Q as private::GetBufferByIndex<'a>>::Queueable: CaptureQueueable<B>,
{
    type Queueable = <Self as private::GetBufferByIndex<'a>>::Queueable;
}

/// Trait for all objects that are capable of providing objects that can be
/// queued to the CAPTURE queue.
pub trait OutputQueueableProvider<'a, B: BufferHandles> {
    type Queueable: OutputQueueable<B>;
}

impl<'a, B, Q> OutputQueueableProvider<'a, B> for Q
where
    B: BufferHandles,
    Q: private::GetBufferByIndex<'a>,
    <Q as private::GetBufferByIndex<'a>>::Queueable: OutputQueueable<B>,
{
    type Queueable = <Self as private::GetBufferByIndex<'a>>::Queueable;
}

pub trait GetOutputBufferByIndex<'a, B, ErrorType = TryGetBufferError>
where
    B: BufferHandles,
    Self: OutputQueueableProvider<'a, B>,
{
    fn try_get_buffer(&'a self, index: usize) -> Result<Self::Queueable, ErrorType>;
}

impl<'a, B: BufferHandles> GetOutputBufferByIndex<'a, B> for Queue<Output, BuffersAllocated<B>>
where
    Self: private::GetBufferByIndex<'a>,
    <Self as private::GetBufferByIndex<'a>>::Queueable: OutputQueueable<B>,
{
    // Take buffer `id` in order to prepare it for queueing, provided it is available.
    fn try_get_buffer(&'a self, index: usize) -> Result<Self::Queueable, TryGetBufferError> {
        <Self as private::GetBufferByIndex<'a>>::try_get_buffer(self, index)
    }
}

pub trait GetCaptureBufferByIndex<'a, P: BufferHandles, ErrorType = TryGetBufferError>
where
    Self: CaptureQueueableProvider<'a, P>,
{
    fn try_get_buffer(&'a self, index: usize) -> Result<Self::Queueable, ErrorType>;
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

pub trait GetFreeOutputBuffer<'a, P: BufferHandles, ErrorType = GetFreeBufferError>
where
    Self: OutputQueueableProvider<'a, P>,
{
    fn try_get_free_buffer(&'a self) -> Result<Self::Queueable, ErrorType>;
}

impl<'a, P: BufferHandles, R, Q> GetFreeOutputBuffer<'a, P> for Q
where
    Self: private::GetFreeBuffer<'a, Queueable = R>,
    Self: OutputQueueableProvider<'a, P, Queueable = R>,
{
    fn try_get_free_buffer(&'a self) -> Result<Self::Queueable, GetFreeBufferError> {
        <Self as private::GetFreeBuffer<'a>>::try_get_free_buffer(self)
    }
}

pub trait GetFreeCaptureBuffer<'a, P: BufferHandles, ErrorType = GetFreeBufferError>
where
    Self: CaptureQueueableProvider<'a, P>,
{
    fn try_get_free_buffer(&'a self) -> Result<Self::Queueable, ErrorType>;
}

impl<'a, P: BufferHandles, R, Q> GetFreeCaptureBuffer<'a, P> for Q
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
    buffer_info: Weak<BufferInfo<P>>,
}

impl<P: BufferHandles> BufferStateFuse<P> {
    /// Create a new fuse that will set `state` to `BufferState::Free` if
    /// destroyed before `disarm()` has been called.
    fn new(buffer_info: Weak<BufferInfo<P>>) -> Self {
        BufferStateFuse { buffer_info }
    }

    /// Disarm this fuse, e.g. the monitored state will be left untouched when
    /// the fuse is destroyed.
    fn disarm(&mut self) {
        // Drop our weak reference.
        self.buffer_info = Weak::new();
    }

    /// Trigger the fuse, i.e. make the buffer return to the Free state, unless the fuse has been
    /// `disarm`ed or the buffer freed. This method should only be called when the reference to the
    /// buffer is being dropped, otherwise inconsistent state may ensue. The fuse will be disarmed
    /// after this call.
    fn trigger(&mut self) {
        match self.buffer_info.upgrade() {
            None => (),
            Some(buffer_info) => {
                buffer_info.update_state(|state| *state = BufferState::Free);
                self.disarm();
            }
        };
    }
}

impl<P: BufferHandles> Drop for BufferStateFuse<P> {
    fn drop(&mut self) {
        self.trigger();
    }
}

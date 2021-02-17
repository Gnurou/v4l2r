use crate::{
    device::{
        poller::{DeviceEvent, PollEvent, Poller, Waker},
        queue::{
            self,
            direction::{Capture, Output},
            dqbuf::DQBuffer,
            handles_provider::HandlesProvider,
            qbuf::{
                get_free::{GetFreeBufferError, GetFreeCaptureBuffer, GetFreeOutputBuffer},
                get_indexed::GetCaptureBufferByIndex,
                CaptureQueueable, OutputQueueableProvider, QueueError,
            },
            BuffersAllocated, CreateQueueError, FormatBuilder, Queue, QueueInit,
            RequestBuffersError,
        },
        AllocatedQueue, Device, DeviceConfig, DeviceOpenError, Stream, TryDequeue,
    },
    ioctl::{self, subscribe_event, BufferCapabilities, Fmt, FormatFlags, StreamOnError},
    memory::{BufferHandles, PrimitiveBufferHandles},
    FormatConversionError,
};

use log::{debug, info, trace, warn};
use std::{
    io,
    path::Path,
    sync::{atomic::AtomicUsize, Arc},
    thread::JoinHandle,
};
use thiserror::Error;

use super::*;

// Trait implemented by all states of the decoder.
pub trait DecoderState {}

pub struct Decoder<S: DecoderState> {
    device: Arc<Device>,
    state: S,
}

pub struct AwaitingOutputFormat {
    output_queue: Queue<Output, QueueInit>,
    capture_queue: Queue<Capture, QueueInit>,
}
impl DecoderState for AwaitingOutputFormat {}

#[derive(Debug, Error)]
pub enum DecoderOpenError {
    #[error("Error while opening device")]
    DeviceOpenError(#[from] DeviceOpenError),
    #[error("Error while creating queue")]
    CreateQueueError(#[from] CreateQueueError),
    #[error("Specified device is not a stateful decoder")]
    NotAStatefulDecoder,
}

impl Decoder<AwaitingOutputFormat> {
    pub fn open(path: &Path) -> Result<Self, DecoderOpenError> {
        let config = DeviceConfig::new().non_blocking_dqbuf();
        let device = Arc::new(Device::open(path, config)?);

        // Check that the device is indeed a stateful decoder.
        let capture_queue = Queue::get_capture_mplane_queue(device.clone())?;
        let output_queue = Queue::get_output_mplane_queue(device.clone())?;

        // On a decoder, the OUTPUT formats are compressed, but the CAPTURE ones are not.
        // Return an error if our device does not satisfy these conditions.
        output_queue
            .format_iter()
            .find(|fmt| fmt.flags.contains(FormatFlags::COMPRESSED))
            .and(
                capture_queue
                    .format_iter()
                    .find(|fmt| !fmt.flags.contains(FormatFlags::COMPRESSED)),
            )
            .ok_or(DecoderOpenError::NotAStatefulDecoder)
            .map(|_| ())?;

        // A stateful decoder won't expose the requests capability on the OUTPUT
        // queue, a stateless one will.
        if output_queue
            .get_capabilities()
            .contains(BufferCapabilities::SUPPORTS_REQUESTS)
        {
            return Err(DecoderOpenError::NotAStatefulDecoder);
        }

        Ok(Decoder {
            device,
            state: AwaitingOutputFormat {
                output_queue,
                capture_queue,
            },
        })
    }

    pub fn set_output_format<F>(mut self, f: F) -> anyhow::Result<Decoder<AwaitingOutputBuffers>>
    where
        F: FnOnce(FormatBuilder) -> anyhow::Result<()>,
    {
        let builder = self.state.output_queue.change_format()?;
        f(builder)?;

        Ok(Decoder {
            device: self.device,
            state: AwaitingOutputBuffers {
                output_queue: self.state.output_queue,
                capture_queue: self.state.capture_queue,
            },
        })
    }
}

pub struct AwaitingOutputBuffers {
    output_queue: Queue<Output, QueueInit>,
    capture_queue: Queue<Capture, QueueInit>,
}
impl DecoderState for AwaitingOutputBuffers {}

impl Decoder<AwaitingOutputBuffers> {
    pub fn allocate_output_buffers_generic<OP: BufferHandles>(
        self,
        memory_type: OP::SupportedMemoryType,
        num_buffers: usize,
    ) -> Result<Decoder<OutputBuffersAllocated<OP>>, RequestBuffersError> {
        Ok(Decoder {
            device: self.device,
            state: OutputBuffersAllocated {
                output_queue: self
                    .state
                    .output_queue
                    .request_buffers_generic::<OP>(memory_type, num_buffers as u32)?,
                capture_queue: self.state.capture_queue,
                poll_wakeups_counter: None,
            },
        })
    }

    pub fn allocate_output_buffers<OP: PrimitiveBufferHandles>(
        self,
        num_output: usize,
    ) -> Result<Decoder<OutputBuffersAllocated<OP>>, RequestBuffersError> {
        self.allocate_output_buffers_generic(OP::MEMORY_TYPE, num_output)
    }
}

pub struct OutputBuffersAllocated<OP: BufferHandles> {
    output_queue: Queue<Output, BuffersAllocated<OP>>,
    capture_queue: Queue<Capture, QueueInit>,
    poll_wakeups_counter: Option<Arc<AtomicUsize>>,
}
impl<OP: BufferHandles> DecoderState for OutputBuffersAllocated<OP> {}

#[derive(Debug, Error)]
pub enum StartDecoderError {
    #[error("IO error")]
    IoError(#[from] io::Error),
    #[error("Cannot subscribe to decoder event")]
    SubscribeEventError(#[from] ioctl::SubscribeEventError),
    #[error("Error while starting the output queue")]
    StreamOnError(#[from] StreamOnError),
}

impl<OP: BufferHandles> Decoder<OutputBuffersAllocated<OP>> {
    pub fn set_poll_counter(mut self, poll_wakeups_counter: Arc<AtomicUsize>) -> Self {
        self.state.poll_wakeups_counter = Some(poll_wakeups_counter);
        self
    }

    #[allow(clippy::type_complexity)]
    pub fn start<P, InputDoneCb, FrameDecodedCb, FormatChangedCb>(
        self,
        input_done_cb: InputDoneCb,
        output_ready_cb: FrameDecodedCb,
        set_capture_format_cb: FormatChangedCb,
    ) -> Result<
        Decoder<Decoding<OP, P, InputDoneCb, FrameDecodedCb, FormatChangedCb>>,
        StartDecoderError,
    >
    where
        P: HandlesProvider,
        InputDoneCb: InputDoneCallback<OP>,
        FrameDecodedCb: FrameDecodedCallback<P>,
        FormatChangedCb: FormatChangedCallback<P>,
        for<'a> Queue<Capture, BuffersAllocated<P::HandleType>>:
            GetFreeCaptureBuffer<'a, P::HandleType> + GetCaptureBufferByIndex<'a, P::HandleType>,
    {
        // We are interested in all resolution change events.
        subscribe_event(
            &*self.device,
            ioctl::EventType::SourceChange,
            ioctl::SubscribeEventFlags::empty(),
        )?;

        let mut output_poller = Poller::new(Arc::clone(&self.device))?;
        output_poller.enable_event(DeviceEvent::OutputReady)?;

        let mut decoder_thread = DecoderThread::new(
            &self.device,
            self.state.capture_queue,
            output_ready_cb,
            set_capture_format_cb,
        )?;

        let stop_waker = Arc::clone(&decoder_thread.stop_waker);

        if let Some(counter) = &self.state.poll_wakeups_counter {
            output_poller.set_poll_counter(Arc::clone(counter));
            decoder_thread.set_poll_counter(Arc::clone(counter));
        }

        let handle = std::thread::Builder::new()
            .name("V4L2 Decoder".into())
            .spawn(move || decoder_thread.run())?;

        self.state.output_queue.stream_on()?;

        Ok(Decoder {
            device: self.device,
            state: Decoding {
                output_queue: self.state.output_queue,
                input_done_cb,
                output_poller,
                stop_waker,
                handle,
            },
        })
    }
}

pub struct Decoding<OP, P, InputDoneCb, FrameDecodedCb, FormatChangedCb>
where
    OP: BufferHandles,
    P: HandlesProvider,
    InputDoneCb: InputDoneCallback<OP>,
    FrameDecodedCb: FrameDecodedCallback<P>,
    FormatChangedCb: FormatChangedCallback<P>,
{
    output_queue: Queue<Output, BuffersAllocated<OP>>,
    input_done_cb: InputDoneCb,
    output_poller: Poller,
    stop_waker: Arc<Waker>,

    handle: JoinHandle<DecoderThread<P, FrameDecodedCb, FormatChangedCb>>,
}
impl<OP, P, InputDoneCb, FrameDecodedCb, FormatChangedCb> DecoderState
    for Decoding<OP, P, InputDoneCb, FrameDecodedCb, FormatChangedCb>
where
    OP: BufferHandles,
    P: HandlesProvider,
    InputDoneCb: InputDoneCallback<OP>,
    FrameDecodedCb: FrameDecodedCallback<P>,
    FormatChangedCb: FormatChangedCallback<P>,
{
}

#[derive(Debug, Error)]
pub enum StopError {
    #[error("Error while poking the stop waker")]
    WakerError(#[from] io::Error),
    #[error("Error while waiting for the decoder thread to finish")]
    JoinError,
    #[error("Error while stopping the OUTPUT queue")]
    StreamoffError(#[from] ioctl::StreamOffError),
}

#[allow(type_alias_bounds)]
type DequeueOutputBufferError<OP: BufferHandles> = ioctl::DQBufError<DQBuffer<Output, OP>>;
#[allow(type_alias_bounds)]
type CanceledBuffers<OP: BufferHandles> =
    Vec<<Queue<Output, BuffersAllocated<OP>> as Stream>::Canceled>;

impl<OP, P, InputDoneCb, FrameDecodedCb, FormatChangedCb>
    Decoder<Decoding<OP, P, InputDoneCb, FrameDecodedCb, FormatChangedCb>>
where
    OP: BufferHandles,
    P: HandlesProvider,
    InputDoneCb: InputDoneCallback<OP>,
    FrameDecodedCb: FrameDecodedCallback<P>,
    FormatChangedCb: FormatChangedCallback<P>,
{
    pub fn num_output_buffers(&self) -> usize {
        self.state.output_queue.num_buffers()
    }

    pub fn get_output_format<E: Into<FormatConversionError>, T: Fmt<E>>(
        &self,
    ) -> Result<T, ioctl::GFmtError> {
        self.state.output_queue.get_format()
    }

    pub fn stop(self) -> Result<CanceledBuffers<OP>, StopError> {
        self.state.stop_waker.wake()?;

        match self.state.handle.join() {
            Ok(_) => (),
            Err(_) => return Err(StopError::JoinError),
        }

        Ok(self.state.output_queue.stream_off()?)
    }

    /// Attempts to dequeue and release output buffers that the driver is done with.
    fn dequeue_output_buffers(&self) -> Result<(), DequeueOutputBufferError<OP>> {
        let output_queue = &self.state.output_queue;

        while output_queue.num_queued_buffers() > 0 {
            match output_queue.try_dequeue() {
                Ok(buf) => {
                    (self.state.input_done_cb)(CompletedInputBuffer::Dequeued(buf));
                }
                Err(ioctl::DQBufError::NotReady) => break,
                // TODO buffers with the error flag set should not result in
                // a fatal error!
                Err(e) => return Err(e),
            }
        }

        Ok(())
    }

    // Make this thread sleep until at least one OUTPUT buffer is ready to be
    // obtained through [`Decoder::try_get_buffer()`].
    fn wait_for_output_buffer(&mut self) -> Result<(), GetBufferError<OP>> {
        for event in self.state.output_poller.poll(None)? {
            match event {
                PollEvent::Device(DeviceEvent::OutputReady) => {
                    self.dequeue_output_buffers()?;
                }
                _ => panic!("Unexpected return from OUTPUT queue poll!"),
            }
        }

        Ok(())
    }
}

impl<'a, OP, P, InputDoneCb, FrameDecodedCb, FormatChangedCb> OutputQueueableProvider<'a, OP>
    for Decoder<Decoding<OP, P, InputDoneCb, FrameDecodedCb, FormatChangedCb>>
where
    Queue<Output, BuffersAllocated<OP>>: OutputQueueableProvider<'a, OP>,
    OP: BufferHandles,
    P: HandlesProvider,
    InputDoneCb: InputDoneCallback<OP>,
    FrameDecodedCb: FrameDecodedCallback<P>,
    FormatChangedCb: FormatChangedCallback<P>,
{
    type Queueable =
        <Queue<Output, BuffersAllocated<OP>> as OutputQueueableProvider<'a, OP>>::Queueable;
}

#[derive(Debug, Error)]
pub enum GetBufferError<OP: BufferHandles> {
    #[error("Error while dequeueing buffer")]
    DequeueError(#[from] DequeueOutputBufferError<OP>),
    #[error("Error during poll")]
    PollError(#[from] io::Error),
    #[error("Error while obtaining buffer")]
    GetFreeBufferError(#[from] GetFreeBufferError),
}

/// Let the decoder provide the buffers from the OUTPUT queue.
impl<'a, OP, P, InputDoneCb, FrameDecodedCb, FormatChangedCb>
    GetFreeOutputBuffer<'a, OP, GetBufferError<OP>>
    for Decoder<Decoding<OP, P, InputDoneCb, FrameDecodedCb, FormatChangedCb>>
where
    Queue<Output, BuffersAllocated<OP>>: GetFreeOutputBuffer<'a, OP>,
    OP: BufferHandles,
    P: HandlesProvider,
    InputDoneCb: InputDoneCallback<OP>,
    FrameDecodedCb: FrameDecodedCallback<P>,
    FormatChangedCb: FormatChangedCallback<P>,
{
    /// Returns a V4L2 buffer to be filled with a frame to decode if one
    /// is available.
    ///
    /// This method will return None immediately if all the allocated buffers
    /// are currently queued.
    fn try_get_free_buffer(&'a self) -> Result<Self::Queueable, GetBufferError<OP>> {
        self.dequeue_output_buffers()?;
        Ok(self.state.output_queue.try_get_free_buffer()?)
    }
}

// If [`GetFreeBuffer`] is implemented, we can also provide a blocking `get_buffer`
// method.
impl<'a, OP, P, InputDoneCb, FrameDecodedCb, FormatChangedCb>
    Decoder<Decoding<OP, P, InputDoneCb, FrameDecodedCb, FormatChangedCb>>
where
    Self: GetFreeOutputBuffer<'a, OP, GetBufferError<OP>>,
    OP: BufferHandles,
    P: HandlesProvider,
    InputDoneCb: InputDoneCallback<OP>,
    FrameDecodedCb: FrameDecodedCallback<P>,
    FormatChangedCb: FormatChangedCallback<P>,
{
    /// Returns a V4L2 buffer to be filled with a frame to encode, waiting for
    /// one to be available if needed.
    ///
    /// Contrary to [`Decoder::try_get_free_buffer()`], this method will wait for a buffer
    /// to be available if needed.
    pub fn get_buffer(
        &'a mut self,
    ) -> Result<<Self as OutputQueueableProvider<'a, OP>>::Queueable, GetBufferError<OP>> {
        let output_queue = &self.state.output_queue;

        // If all our buffers are queued, wait until we can dequeue some.
        if output_queue.num_queued_buffers() == output_queue.num_buffers() {
            self.wait_for_output_buffer()?;
        }

        self.try_get_free_buffer()
    }

    /// Kick the decoder and see if some input buffers fall as a result.
    ///
    /// No, really. Completed input buffers are typically checked when calling
    /// [`get_buffer`] (which is also the time when the input done callback is
    /// invoked), but this mechanism is not foolproof: if the client works with
    /// a limited set of input buffers and queues them all before an output
    /// frame can be produced, then the client has no more buffers to fill and
    /// thus no reason to call [`get_buffer`], resulting in the decoding process
    /// being blocked.
    ///
    /// This method mitigates this problem by adding a way to check for
    /// completed input buffers and calling the input done callback without the
    /// need for new encoded content. It is suggested to call it from the thread
    /// that owns the decoder every time a decoded frame is produced.
    /// That way the client can recycle its input buffers
    /// and the decoding process does not get stuck.
    pub fn kick(&mut self) -> Result<(), DequeueOutputBufferError<OP>> {
        info!("Kick!");
        self.dequeue_output_buffers()
    }

    pub fn wait_for_input_available(&mut self) -> Result<(), GetBufferError<OP>> {
        let output_queue = &self.state.output_queue;

        // If all our buffers are queued, wait until we can dequeue some.
        if output_queue.num_queued_buffers() == output_queue.num_buffers() {
            self.wait_for_output_buffer()?;
            // Dequeue and run the input done callback for completed input buffers.
            self.dequeue_output_buffers()?;
        }

        Ok(())
    }
}

// TODO use ::new functions that take the queue and configure the state properly, with
// the poller, wakers, and all.
enum CaptureQueue<P: HandlesProvider> {
    AwaitingResolution {
        capture_queue: Queue<Capture, QueueInit>,
    },
    Decoding {
        capture_queue: Queue<Capture, BuffersAllocated<P::HandleType>>,
        provider: P,
        cap_buffer_waker: Arc<Waker>,
    },
}

struct DecoderThread<P, FrameDecodedCb, FormatChangedCb>
where
    P: HandlesProvider,
    FrameDecodedCb: FrameDecodedCallback<P>,
    FormatChangedCb: FormatChangedCallback<P>,
{
    device: Arc<Device>,
    capture_queue: CaptureQueue<P>,
    poller: Poller,
    stop_waker: Arc<Waker>,
    output_ready_cb: FrameDecodedCb,
    set_capture_format_cb: FormatChangedCb,
}

#[derive(Debug, Error)]
enum UpdateCaptureError {
    #[error("Error while enabling poller events: {0}")]
    PollerEvents(io::Error),
    #[error("Error while removing CAPTURE waker: {0}")]
    RemoveWaker(io::Error),
    #[error("Error while stopping CAPTURE queue: {0}")]
    Streamoff(#[from] ioctl::StreamOffError),
    #[error("Error while freeing CAPTURE buffers: {0}")]
    FreeBuffers(#[from] ioctl::ReqbufsError),
    #[error("Error while obtaining CAPTURE format: {0}")]
    GFmt(#[from] ioctl::GFmtError),
    #[error("Error while running the CAPTURE format callback: {0}")]
    Callback(#[from] anyhow::Error),
    #[error("Error while requesting CAPTURE buffers: {0}")]
    RequestBuffers(#[from] queue::RequestBuffersError),
    #[error("Error while adding the CAPTURE buffer waker: {0}")]
    AddWaker(io::Error),
    #[error("Error while signaling the CAPTURE buffer waker: {0}")]
    WakeWaker(io::Error),
    #[error("Error while streaming CAPTURE queue: {0}")]
    StreamOn(#[from] ioctl::StreamOnError),
}

const CAPTURE_READY: u32 = 0;
const STOP_DECODING: u32 = 1;

#[derive(Debug, Error)]
enum ProcessEventsError {
    #[error("Error while dequeueing event")]
    DQEvent(#[from] ioctl::DQEventError),
    #[error("Error while requesting buffers")]
    RequestBuffers(#[from] queue::RequestBuffersError),
    #[error("Error while updating CAPTURE format")]
    UpdateCapture(#[from] UpdateCaptureError),
}

impl<P, FrameDecodedCb, FormatChangedCb> DecoderThread<P, FrameDecodedCb, FormatChangedCb>
where
    P: HandlesProvider,
    FrameDecodedCb: FrameDecodedCallback<P>,
    FormatChangedCb: FormatChangedCallback<P>,
    for<'a> Queue<Capture, BuffersAllocated<P::HandleType>>:
        GetFreeCaptureBuffer<'a, P::HandleType> + GetCaptureBufferByIndex<'a, P::HandleType>,
{
    fn new(
        device: &Arc<Device>,
        capture_queue: Queue<Capture, QueueInit>,
        output_ready_cb: FrameDecodedCb,
        set_capture_format_cb: FormatChangedCb,
    ) -> io::Result<Self> {
        // Start by only listening to V4L2 events in order to catch the initial
        // resolution change, and to the stop waker in case the user had a
        // change of heart about decoding something now.
        let mut poller = Poller::new(Arc::clone(device))?;
        poller.enable_event(DeviceEvent::V4L2Event)?;
        let stop_waker = poller.add_waker(STOP_DECODING)?;

        let decoder_thread = DecoderThread {
            device: Arc::clone(&device),
            capture_queue: CaptureQueue::AwaitingResolution { capture_queue },
            poller,
            stop_waker,
            output_ready_cb,
            set_capture_format_cb,
        };

        Ok(decoder_thread)
    }

    fn set_poll_counter(&mut self, poll_wakeups_counter: Arc<AtomicUsize>) {
        self.poller.set_poll_counter(poll_wakeups_counter);
    }

    fn update_capture_format(mut self) -> Result<Self, UpdateCaptureError> {
        debug!("Updating CAPTURE format");
        // First reset the capture queue to the `Init` state if needed.
        let mut capture_queue = match self.capture_queue {
            // Initial resolution
            CaptureQueue::AwaitingResolution { capture_queue } => {
                // Stop listening to V4L2 events. We will check them when we get
                // a buffer with the LAST flag.
                self.poller
                    .disable_event(DeviceEvent::V4L2Event)
                    .map_err(UpdateCaptureError::PollerEvents)?;
                // Listen to CAPTURE buffers being ready to dequeue, as we will
                // be streaming soon.
                self.poller
                    .enable_event(DeviceEvent::CaptureReady)
                    .map_err(UpdateCaptureError::PollerEvents)?;
                capture_queue
            }
            // Dynamic resolution change
            CaptureQueue::Decoding { capture_queue, .. } => {
                // Remove the waker for the previous buffers pool, as we will
                // get a new set of buffers.
                self.poller
                    .remove_waker(CAPTURE_READY)
                    .map_err(UpdateCaptureError::RemoveWaker)?;
                // Deallocate the queue and return it to the `Init` state. Good
                // as new!
                capture_queue.stream_off()?;
                capture_queue.free_buffers()?.queue
            }
        };

        // Now get the parameters of the new format and build our new CAPTURE
        // queue.

        // TODO use the proper control to get the right value.
        let min_num_buffers = 4usize;

        debug!("Stream requires {} capture buffers", min_num_buffers);

        // Let the client adjust the new format and give us the handles provider.
        let FormatChangedReply {
            provider,
            mem_type,
            num_buffers,
        } = (self.set_capture_format_cb)(capture_queue.change_format()?, min_num_buffers)?;

        debug!("Client requires {} capture buffers", num_buffers);

        // Allocate the new CAPTURE buffers and get ourselves a new waker for
        // returning buffers.
        let capture_queue =
            capture_queue.request_buffers_generic::<P::HandleType>(mem_type, num_buffers as u32)?;
        let cap_buffer_waker = self
            .poller
            .add_waker(CAPTURE_READY)
            .map_err(UpdateCaptureError::AddWaker)?;

        // Ready to decode - signal the waker so we immediately enqueue buffers
        // and start streaming.
        cap_buffer_waker
            .wake()
            .map_err(UpdateCaptureError::WakeWaker)?;
        capture_queue.stream_on()?;

        Ok(Self {
            capture_queue: CaptureQueue::Decoding {
                capture_queue,
                provider,
                cap_buffer_waker,
            },
            ..self
        })
    }

    /// Check if we have a dynamic resolution change event pending.
    ///
    /// Dequeues all pending V4L2 events and returns `true` if a
    /// SRC_CHANGE_EVENT (indicating a format change on the CAPTURE queue) was
    /// detected. This consumes all the event, meaning that if this method
    /// returned `true` once it will return `false` until a new resolution
    /// change happens in the stream.
    fn is_drc_event_pending(&self) -> Result<bool, ioctl::DQEventError> {
        let mut drc_pending = false;

        loop {
            // TODO what if we used an iterator here?
            let event = match ioctl::dqevent(&*self.device) {
                Ok(event) => event,
                Err(ioctl::DQEventError::NotReady) => return Ok(drc_pending),
                Err(e) => return Err(e),
            };

            match event {
                ioctl::Event::SrcChangeEvent(changes) => {
                    if changes.contains(ioctl::SrcChanges::RESOLUTION) {
                        debug!("Received resolution change event");
                        drc_pending = true;
                    }
                }
            }
        }
    }

    /// Try to dequeue and process a single CAPTURE buffer.
    ///
    /// Returns `true` if the buffer had the LAST flag set, `false` otherwise.
    fn process_capture_buffer(&mut self) -> bool {
        if let CaptureQueue::Decoding {
            capture_queue,
            cap_buffer_waker,
            ..
        } = &mut self.capture_queue
        {
            match capture_queue.try_dequeue() {
                Ok(mut cap_buf) => {
                    let is_last = cap_buf.data.flags().contains(ioctl::BufferFlags::LAST);

                    // Add a drop callback to the dequeued buffer so we
                    // re-queue it as soon as it is dropped.
                    let cap_waker = Arc::clone(&cap_buffer_waker);
                    cap_buf.add_drop_callback(move |_dqbuf| {
                        // Intentionally ignore the result here.
                        let _ = cap_waker.wake();
                    });

                    // Pass buffers to the client
                    (self.output_ready_cb)(cap_buf);
                    is_last
                }
                Err(e) => {
                    warn!(
                        "Expected a CAPTURE buffer but none available, possible driver bug: {}",
                        e
                    );
                    false
                }
            }
        } else {
            // TODO replace with something more elegant.
            panic!();
        }
    }

    fn run(mut self) -> Self {
        'polling: loop {
            match &self.capture_queue {
                CaptureQueue::AwaitingResolution { .. } => {
                    // TODO remove this unwrap.
                    trace!("AwaitingResolution: polling...");
                    for event in self.poller.poll(None).unwrap() {
                        match event {
                            // Check if we got a format change event (ignoring
                            // other events), and switch to the `Decoding` state
                            // if we do.
                            PollEvent::Device(DeviceEvent::V4L2Event) => {
                                trace!("AwaitingResolution: got V4L2Event");
                                if self.is_drc_event_pending().unwrap() {
                                    self = self.update_capture_format().unwrap()
                                }
                            }
                            // If we are requested to stop, then we just need to
                            // break the loop since we haven't started producing
                            // buffers.
                            PollEvent::Waker(STOP_DECODING) => {
                                trace!("AwaitingResolution: got STOP_DECODING waker");
                                break 'polling;
                            }
                            _ => panic!("Unexpected event!"),
                        }
                    }
                }
                CaptureQueue::Decoding { capture_queue, .. } => {
                    match capture_queue.num_queued_buffers() {
                        // If there are no buffers on the CAPTURE queue, poll() will return
                        // immediately with EPOLLERR and we would loop indefinitely.
                        // Prevent this by temporarily disabling polling the CAPTURE queue
                        // in such cases.
                        0 => {
                            self.poller
                                .disable_event(DeviceEvent::CaptureReady)
                                .unwrap();
                        }
                        // If device polling was disabled and we have buffers queued, we
                        // can reenable it as poll will now wait for a CAPTURE buffer to
                        // be ready for dequeue.
                        _ => {
                            self.poller.enable_event(DeviceEvent::CaptureReady).unwrap();
                        }
                    }

                    trace!("Decoding: polling...");
                    // TODO remove this unwrap.
                    for event in self.poller.poll(None).unwrap() {
                        match event {
                            PollEvent::Device(DeviceEvent::CaptureReady) => {
                                trace!("Decoding: got CaptureReady");
                                let is_last = self.process_capture_buffer();
                                if is_last {
                                    debug!("CAPTURE buffer marked with LAST flag");
                                    if self.is_drc_event_pending().unwrap() {
                                        self = self.update_capture_format().unwrap();
                                    }
                                    // No DRC event pending, this is the end of the stream.
                                    // We cannot keep polling anymore since the `CaptureReady`
                                    // event will keep being signaled, but dequeuing will only
                                    // return `EPIPE`.
                                    else {
                                        debug!("No DRC event pending, exiting decoding loop");
                                        break 'polling;
                                    }
                                }
                            }
                            PollEvent::Waker(CAPTURE_READY) => {
                                trace!("Decoding: got CAPTURE_READY waker");
                                // Requeue all available CAPTURE buffers.
                                self.enqueue_capture_buffers().unwrap();
                            }
                            PollEvent::Waker(STOP_DECODING) => {
                                trace!("Decoding: got STOP_DECODING waker");
                                // We are already producing buffers, send the STOP command
                                // and exit the loop once the buffer with the LAST tag is received.
                                // TODO remove this unwrap.
                                ioctl::decoder_cmd(&*self.device, ioctl::DecoderCommand::Stop)
                                    .unwrap();
                            }
                            _ => panic!("Unexpected event!"),
                        }
                    }
                }
            }
        }

        // Return the decoder to the awaiting resolution state.
        match self.capture_queue {
            CaptureQueue::AwaitingResolution { .. } => self,
            CaptureQueue::Decoding { capture_queue, .. } => Self {
                capture_queue: CaptureQueue::AwaitingResolution {
                    capture_queue: {
                        capture_queue.stream_off().unwrap();
                        capture_queue.free_buffers().unwrap().queue
                    },
                },
                poller: {
                    let mut poller = self.poller;
                    poller.disable_event(DeviceEvent::CaptureReady).unwrap();
                    poller.enable_event(DeviceEvent::V4L2Event).unwrap();
                    poller.remove_waker(CAPTURE_READY).unwrap();
                    poller
                },
                ..self
            },
        }
    }

    fn enqueue_capture_buffers(&mut self) -> Result<(), QueueError<P::HandleType>> {
        if let CaptureQueue::Decoding {
            capture_queue,
            provider,
            cap_buffer_waker,
        } = &mut self.capture_queue
        {
            'enqueue: while let Some(handles) = provider.get_handles(&cap_buffer_waker) {
                // TODO potential problem: the handles will be dropped if no V4L2 buffer
                // is available. There is no guarantee that the provider will get them back
                // in this case (e.g. with the C FFI).
                if let Ok(buffer) = provider.get_suitable_buffer_for(&handles, capture_queue) {
                    buffer.queue_with_handles(handles)?;
                } else {
                    warn!("Handles potentially lost due to no V4L2 buffer being available");
                    break 'enqueue;
                };
            }
        }

        Ok(())
    }
}

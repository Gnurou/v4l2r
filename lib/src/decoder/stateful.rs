use crate::{
    device::{
        poller::{DeviceEvent, PollError, PollEvent, Poller, Waker},
        queue::{
            self,
            direction::{Capture, Output},
            dqbuf::DQBuffer,
            handles_provider::HandlesProvider,
            qbuf::{
                get_free::{GetFreeBufferError, GetFreeCaptureBuffer, GetFreeOutputBuffer},
                get_indexed::GetCaptureBufferByIndex,
                CaptureQueueable, OutputQueueableProvider,
            },
            BuffersAllocated, CreateQueueError, FormatBuilder, Queue, QueueInit,
            RequestBuffersError,
        },
        AllocatedQueue, Device, DeviceConfig, DeviceOpenError, Stream, TryDequeue,
    },
    ioctl::{
        self, subscribe_event, BufferCapabilities, Fmt, FormatFlags, SelectionTarget, StreamOnError,
    },
    memory::{BufferHandles, PrimitiveBufferHandles},
    FormatConversionError,
};

use log::{debug, error, info, trace, warn};
use std::{
    io,
    path::Path,
    sync::{atomic::AtomicUsize, mpsc, Arc},
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

        let (command_sender, command_receiver) = mpsc::channel::<DecoderCommand>();
        let (response_sender, response_receiver) = mpsc::channel::<CaptureThreadResponse>();

        let mut decoder_thread = DecoderThread::new(
            &self.device,
            self.state.capture_queue,
            output_ready_cb,
            set_capture_format_cb,
            command_receiver,
            response_sender,
        )?;

        let command_waker = Arc::clone(&decoder_thread.command_waker);

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
                command_waker,
                command_sender,
                response_receiver,
                handle,
            },
        })
    }
}

enum DecoderCommand {
    Drain,
    Flush,
    Stop,
}

enum CaptureThreadResponse {
    DrainDone(anyhow::Result<bool>),
    FlushDone(anyhow::Result<()>),
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

    command_waker: Arc<Waker>,
    command_sender: mpsc::Sender<DecoderCommand>,
    response_receiver: mpsc::Receiver<CaptureThreadResponse>,

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
pub enum SendCommandError {
    #[error("Error while queueing the message")]
    SendError,
    #[error("Error while poking the command waker")]
    WakerError(#[from] io::Error),
}

#[derive(Debug, Error)]
pub enum StopError {
    #[error("Error while sending the stop command to the capture thread")]
    SendCommand(#[from] SendCommandError),
    #[error("Error while waiting for the decoder thread to finish")]
    Join,
    #[error("Error while stopping the OUTPUT queue")]
    Streamoff(#[from] ioctl::StreamOffError),
}

#[derive(Debug, Error)]
pub enum DrainError {
    #[error("Error while sending the flush command to the capture thread")]
    SendCommand(#[from] SendCommandError),
    #[error("Error while waiting for the decoder thread to drain")]
    RecvError(#[from] mpsc::RecvError),
    #[error("Error while draining on the capture thread")]
    CaptureThreadError(anyhow::Error),
}

#[derive(Debug, Error)]
pub enum FlushError {
    #[error("Error while stopping the OUTPUT queue")]
    StreamoffError(#[from] ioctl::StreamOffError),
    #[error("Error while sending the flush command to the capture thread")]
    SendCommand(#[from] SendCommandError),
    #[error("Error while waiting for the decoder thread to flush")]
    RecvError(#[from] mpsc::RecvError),
    #[error("Error while flushing on the capture thread")]
    CaptureThreadError(anyhow::Error),
    #[error("Error while starting the OUTPUT queue")]
    StreamonError(#[from] ioctl::StreamOnError),
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

    /// Send a command to the capture thread.
    fn send_command(&self, command: DecoderCommand) -> Result<(), SendCommandError> {
        self.state
            .command_sender
            .send(command)
            .map_err(|_| SendCommandError::SendError)?;
        self.state.command_waker.wake()?;

        Ok(())
    }

    pub fn get_output_format<E: Into<FormatConversionError>, T: Fmt<E>>(
        &self,
    ) -> Result<T, ioctl::GFmtError> {
        self.state.output_queue.get_format()
    }

    /// Stop the decoder.
    ///
    /// This will stop any pending operation consume the decoder, which cannot
    /// be used anymore. To make sure all submitted encoded buffers have been
    /// processed, call the [`drain`] method and wait for the output buffer with
    /// the LAST flag before calling this method.
    ///
    /// TODO potential bug: the LAST buffer could also be the one signaling a
    /// DRC. We need another way to manage this? Probably a good idea to split
    /// into two properties of DQBuf.
    pub fn stop(self) -> Result<CanceledBuffers<OP>, StopError> {
        debug!("Stop requested");
        self.send_command(DecoderCommand::Stop)?;

        match self.state.handle.join() {
            Ok(_) => (),
            Err(_) => return Err(StopError::Join),
        }

        Ok(self.state.output_queue.stream_off()?)
    }

    /// Drain the decoder, i.e. make sure all its pending work is processed and
    /// signal this with a buffer carrying the LAST flag.
    ///
    /// This method is usually non-blocking: because the decoding process must
    /// continue until the LAST buffer is received, it is up to the client to
    /// watch for that buffer and only consider the drain completed when it
    /// arrives.
    ///
    /// This method returns `false` to indicate that the client must look for
    /// the LAST buffer before it can consider the drain sequence as completed.
    /// However, if the decoder is still not able to emit frames at this stage,
    /// making it impossible to receive the LAST buffer, this method will return
    /// `true` and the drain sequence can be considered as completed.
    ///
    /// The client can continue submitting buffers with encoded data. They will
    /// be processed in order and their frames will come after the ones still
    /// in the pipeline. For a way to cancel all the pending jobs, see the
    /// [`flush`] method.
    pub fn drain(&self) -> Result<bool, DrainError> {
        debug!("Drain requested");
        self.send_command(DecoderCommand::Drain)?;

        match self.state.response_receiver.recv()? {
            CaptureThreadResponse::DrainDone(response) => match response {
                Ok(completed) => Ok(completed),
                Err(e) => {
                    error!("Error while draining on the capture thread: {}", e);
                    Err(DrainError::CaptureThreadError(e))
                }
            },
            _ => {
                error!("Unexpected capture thread response received while draining");
                Err(DrainError::CaptureThreadError(anyhow::anyhow!(
                    "Unexpected response while draining"
                )))
            }
        }
    }

    /// Flush the decoder, i.e. try to cancel all pending work.
    ///
    /// The canceled input buffers will be returned as
    /// `CompletedInputBuffer::Canceled` through the input done callback.
    ///
    /// This function is blocking. When is returns, the frame decoded callback
    /// has been called for all pre-flush frames, and the decoder can accept new
    /// content to decode.
    pub fn flush(&self) -> Result<(), FlushError> {
        debug!("Flush requested");
        let canceled_buffers = self.state.output_queue.stream_off()?;

        // Request the decoder to flush itself.
        self.send_command(DecoderCommand::Flush)?;

        // Process our canceled input buffers in the meantime.
        for buffer in canceled_buffers {
            (self.state.input_done_cb)(CompletedInputBuffer::Canceled(buffer));
        }

        // Wait for the decoder thread to signal it is done with our request.
        // TODO add timeout?
        match self.state.response_receiver.recv()? {
            CaptureThreadResponse::FlushDone(response) => match response {
                Ok(()) => (),
                Err(e) => {
                    error!("Error while flushing on the capture thread: {}", e);
                    return Err(FlushError::CaptureThreadError(e));
                }
            },
            _ => {
                error!("Unexpected capture thread response received while flushing");
                return Err(FlushError::CaptureThreadError(anyhow::anyhow!(
                    "Unexpected response while flushing"
                )));
            }
        }

        // Resume business.
        self.state.output_queue.stream_on()?;

        debug!("Flush complete");
        Ok(())
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
    PollError(#[from] PollError),
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
    /// [`Decoder::get_buffer`] (which is also the time when the input done callback is
    /// invoked), but this mechanism is not foolproof: if the client works with
    /// a limited set of input buffers and queues them all before an output
    /// frame can be produced, then the client has no more buffers to fill and
    /// thus no reason to call [`Decoder::get_buffer`], resulting in the decoding process
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

    // Switched when we need the capture thread to quit and return the decoder
    // to its initial state.
    stop_flag: bool,

    output_ready_cb: FrameDecodedCb,
    set_capture_format_cb: FormatChangedCb,

    // Waker signaled when the main thread has commands pending for us.
    command_waker: Arc<Waker>,
    // Receiver we read commands from when `command_waker` is signaled.
    command_receiver: mpsc::Receiver<DecoderCommand>,
    // Sender we use to send status messages after receiving commands from the
    // main thread.
    response_sender: mpsc::Sender<CaptureThreadResponse>,
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
    #[error("Error while obtaining selection target from CAPTURE queue: {0}")]
    GSelection(#[from] ioctl::GSelectionError),
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

const CAPTURE_READY: u32 = 1;
const COMMAND_WAITING: u32 = 2;

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
        command_receiver: mpsc::Receiver<DecoderCommand>,
        response_sender: mpsc::Sender<CaptureThreadResponse>,
    ) -> io::Result<Self> {
        // Start by only listening to V4L2 events in order to catch the initial
        // resolution change, and to the stop waker in case the user had a
        // change of heart about decoding something now.
        let mut poller = Poller::new(Arc::clone(device))?;
        poller.enable_event(DeviceEvent::V4L2Event)?;
        let command_waker = poller.add_waker(COMMAND_WAITING)?;

        let decoder_thread = DecoderThread {
            device: Arc::clone(&device),
            capture_queue: CaptureQueue::AwaitingResolution { capture_queue },
            poller,
            stop_flag: false,
            output_ready_cb,
            set_capture_format_cb,
            command_waker,
            command_receiver,
            response_sender,
        };

        Ok(decoder_thread)
    }

    fn set_poll_counter(&mut self, poll_wakeups_counter: Arc<AtomicUsize>) {
        self.poller.set_poll_counter(poll_wakeups_counter);
    }

    fn stop(&mut self) {
        trace!("Processing stop command");
        self.stop_flag = true;
    }

    fn drain(&mut self) {
        trace!("Processing drain command");
        let response = match self.capture_queue {
            CaptureQueue::AwaitingResolution { .. } => true,
            CaptureQueue::Decoding { .. } => {
                // We can receive the LAST buffer, send the STOP command
                // and exit the loop once the buffer with the LAST tag is received.
                ioctl::decoder_cmd(&*self.device, ioctl::DecoderCommand::Stop).unwrap();
                false
            }
        };

        self.response_sender
            .send(CaptureThreadResponse::DrainDone(Ok(response)))
            .unwrap();
    }

    fn enqueue_capture_buffers(&mut self) {
        trace!("Queueing available CAPTURE buffers");
        match &mut self.capture_queue {
            CaptureQueue::AwaitingResolution { .. } => unreachable!(),
            CaptureQueue::Decoding {
                capture_queue,
                provider,
                cap_buffer_waker,
            } => {
                // Requeue all available CAPTURE buffers.
                'enqueue: while let Some(handles) = provider.get_handles(&cap_buffer_waker) {
                    // TODO potential problem: the handles will be dropped if no V4L2 buffer
                    // is available. There is no guarantee that the provider will get them back
                    // in this case (e.g. with the C FFI).
                    if let Ok(buffer) = provider.get_suitable_buffer_for(&handles, capture_queue) {
                        buffer.queue_with_handles(handles).unwrap();
                    } else {
                        warn!("Handles potentially lost due to no V4L2 buffer being available");
                        break 'enqueue;
                    };
                }
            }
        }
    }

    fn process_v4l2_event(mut self) -> Self {
        trace!("Processing V4L2 event");
        match self.capture_queue {
            CaptureQueue::AwaitingResolution { .. } => {
                if self.is_drc_event_pending().unwrap() {
                    self = self.update_capture_format().unwrap()
                }
            }
            CaptureQueue::Decoding { .. } => {
                // TODO normally we shouldn't listen to events while in Decoding
                // state. We do this as a workaround for virtio-video. Remove
                // this block once the issue is fixed in that driver and replace
                // it with unreachable!().
                if self.is_drc_event_pending().unwrap() {
                    warn!("Detected DRC during decode.");
                    for _ in 0..16 {
                        self.process_capture_buffer();
                    }
                    self = self.update_capture_format().unwrap()
                }
            }
        }

        self
    }

    fn update_capture_format(mut self) -> Result<Self, UpdateCaptureError> {
        debug!("Updating CAPTURE format");
        // First reset the capture queue to the `Init` state if needed.
        let mut capture_queue = match self.capture_queue {
            // Initial resolution
            CaptureQueue::AwaitingResolution { capture_queue } => {
                // Keep listening to DRC event as workaround for virtio-video.
                /*
                // Stop listening to V4L2 events. We will check them when we get
                // a buffer with the LAST flag.
                self.poller
                    .disable_event(DeviceEvent::V4L2Event)
                    .map_err(UpdateCaptureError::PollerEvents)?;
                */
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

        let visible_rect = capture_queue.get_selection(SelectionTarget::Compose)?;
        debug!(
            "Visible rectangle: ({}, {}), {}x{}",
            visible_rect.left, visible_rect.top, visible_rect.width, visible_rect.height
        );

        // Let the client adjust the new format and give us the handles provider.
        let FormatChangedReply {
            provider,
            mem_type,
            num_buffers,
        } = (self.set_capture_format_cb)(
            capture_queue.change_format()?,
            visible_rect,
            min_num_buffers,
        )?;

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

    fn dequeue_capture_buffers(mut self) -> Self {
        trace!("Dequeueing decoded CAPTURE buffers");
        match self.capture_queue {
            CaptureQueue::AwaitingResolution { .. } => unreachable!(),
            CaptureQueue::Decoding { .. } => {
                let is_last = self.process_capture_buffer();
                if is_last {
                    debug!("CAPTURE buffer marked with LAST flag");
                    if self.is_drc_event_pending().unwrap() {
                        self = self.update_capture_format().unwrap()
                    }
                    // No DRC event pending, this is the end of the stream.
                    // We need to stop and restart the CAPTURE queue, otherwise
                    // it will keep signaling buffers as ready and dequeueing
                    // them will return `EPIPE`.
                    else {
                        debug!("No DRC event pending, restarting capture queue");

                        // We are supposed to be able to run the START command
                        // instead, but with vicodec the CAPTURE queue reports
                        // as ready in subsequent polls() and DQBUF returns
                        // -EPIPE...
                        self.restart_capture_queue();
                    }
                }
            }
        }
        self
    }

    /// Stream the capture queue off and back on, dropping any queued buffer,
    /// and making the decoder ready to work again if it was halted.
    fn restart_capture_queue(&mut self) {
        match &self.capture_queue {
            CaptureQueue::AwaitingResolution { .. } => {}
            CaptureQueue::Decoding { capture_queue, .. } => {
                capture_queue.stream_off().unwrap();
                capture_queue.stream_on().unwrap();
            }
        }
    }

    fn flush(&mut self) {
        trace!("Processing flush command");
        self.restart_capture_queue();

        // We are flushed, let the client know.
        self.response_sender
            .send(CaptureThreadResponse::FlushDone(Ok(())))
            .unwrap();

        self.enqueue_capture_buffers()
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
        'mainloop: while !self.stop_flag {
            if let CaptureQueue::Decoding { capture_queue, .. } = &self.capture_queue {
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
            }

            trace!("Polling...");
            let events = match self.poller.poll(None) {
                Ok(events) => events,
                Err(e) => {
                    error!("Polling failure, exiting capture thread: {}", e);
                    break 'mainloop;
                }
            };
            for event in events {
                self = match event {
                    PollEvent::Device(DeviceEvent::V4L2Event) => self.process_v4l2_event(),
                    PollEvent::Device(DeviceEvent::CaptureReady) => self.dequeue_capture_buffers(),
                    PollEvent::Waker(CAPTURE_READY) => {
                        self.enqueue_capture_buffers();
                        self
                    }
                    PollEvent::Waker(COMMAND_WAITING) => {
                        loop {
                            let command =
                                match self.command_receiver.recv_timeout(Default::default()) {
                                    Ok(command) => command,
                                    Err(mpsc::RecvTimeoutError::Timeout) => break,
                                    Err(e) => {
                                        error!("Error while reading decoder command: {}", e);
                                        break;
                                    }
                                };
                            match command {
                                DecoderCommand::Drain => self.drain(),
                                DecoderCommand::Flush => self.flush(),
                                DecoderCommand::Stop => self.stop(),
                            }
                        }
                        self
                    }
                    _ => panic!("Unexpected event!"),
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
}

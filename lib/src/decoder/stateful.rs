mod capture_thread;

use crate::{
    device::{
        poller::{DeviceEvent, PollError, PollEvent, Poller, Waker},
        queue::{
            direction::{Capture, Output},
            dqbuf::DqBuffer,
            handles_provider::HandlesProvider,
            qbuf::{
                get_free::{GetFreeBufferError, GetFreeCaptureBuffer, GetFreeOutputBuffer},
                get_indexed::GetCaptureBufferByIndex,
                OutputQueueableProvider,
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

use capture_thread::CaptureThread;
use log::{debug, error, info, trace};
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
    ) -> Result<Decoder<ReadyToDecode<OP>>, RequestBuffersError> {
        Ok(Decoder {
            device: self.device,
            state: ReadyToDecode {
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
    ) -> Result<Decoder<ReadyToDecode<OP>>, RequestBuffersError> {
        self.allocate_output_buffers_generic(OP::MEMORY_TYPE, num_output)
    }
}

pub struct ReadyToDecode<OP: BufferHandles> {
    output_queue: Queue<Output, BuffersAllocated<OP>>,
    capture_queue: Queue<Capture, QueueInit>,
    poll_wakeups_counter: Option<Arc<AtomicUsize>>,
}
impl<OP: BufferHandles> DecoderState for ReadyToDecode<OP> {}

#[derive(Debug, Error)]
pub enum StartDecoderError {
    #[error("Error while creating poller")]
    CannotCreatePoller(nix::Error),
    #[error("Cannot subscribe to decoder event")]
    SubscribeEventError(#[from] ioctl::SubscribeEventError),
    #[error("Error while enabling event")]
    CannotEnableEvent(nix::Error),
    #[error("Error while creating capture thread")]
    CannotCreateCaptureThread(io::Error),
    #[error("Error while activating capture thread")]
    CannotStartCaptureThread(io::Error),
    #[error("Error while starting the output queue")]
    StreamOnError(#[from] StreamOnError),
}

impl<OP: BufferHandles> Decoder<ReadyToDecode<OP>> {
    pub fn set_poll_counter(mut self, poll_wakeups_counter: Arc<AtomicUsize>) -> Self {
        self.state.poll_wakeups_counter = Some(poll_wakeups_counter);
        self
    }

    #[allow(clippy::type_complexity)]
    pub fn start<P, InputDoneCb, DecoderEventCb, FormatChangedCb>(
        self,
        input_done_cb: InputDoneCb,
        decoder_event_cb: DecoderEventCb,
        set_capture_format_cb: FormatChangedCb,
    ) -> Result<
        Decoder<Decoding<OP, P, InputDoneCb, DecoderEventCb, FormatChangedCb>>,
        StartDecoderError,
    >
    where
        P: HandlesProvider,
        InputDoneCb: InputDoneCallback<OP>,
        DecoderEventCb: DecoderEventCallback<P>,
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

        let mut output_poller =
            Poller::new(Arc::clone(&self.device)).map_err(StartDecoderError::CannotCreatePoller)?;
        output_poller
            .enable_event(DeviceEvent::OutputReady)
            .map_err(StartDecoderError::CannotEnableEvent)?;

        let (command_sender, command_receiver) = mpsc::channel::<DecoderCommand>();
        let (response_sender, response_receiver) = mpsc::channel::<CaptureThreadResponse>();

        let mut decoder_thread = CaptureThread::new(
            &self.device,
            self.state.capture_queue,
            decoder_event_cb,
            set_capture_format_cb,
            command_receiver,
            response_sender,
        )
        .map_err(StartDecoderError::CannotCreateCaptureThread)?;

        let command_waker = Arc::clone(&decoder_thread.command_waker);

        if let Some(counter) = &self.state.poll_wakeups_counter {
            output_poller.set_poll_counter(Arc::clone(counter));
            decoder_thread.poller.set_poll_counter(Arc::clone(counter));
        }

        let handle = std::thread::Builder::new()
            .name("V4L2 Decoder".into())
            .spawn(move || decoder_thread.run())
            .map_err(StartDecoderError::CannotStartCaptureThread)?;

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

#[derive(Debug)]
enum DecoderCommand {
    Drain(bool),
    Flush,
    Stop,
}

#[derive(Debug)]
enum CaptureThreadResponse {
    DrainDone(Result<bool, DrainError>),
    FlushDone(anyhow::Result<()>),
}

pub struct Decoding<OP, P, InputDoneCb, DecoderEventCb, FormatChangedCb>
where
    OP: BufferHandles,
    P: HandlesProvider,
    InputDoneCb: InputDoneCallback<OP>,
    DecoderEventCb: DecoderEventCallback<P>,
    FormatChangedCb: FormatChangedCallback<P>,
{
    output_queue: Queue<Output, BuffersAllocated<OP>>,
    input_done_cb: InputDoneCb,
    output_poller: Poller,

    command_waker: Arc<Waker>,
    command_sender: mpsc::Sender<DecoderCommand>,
    response_receiver: mpsc::Receiver<CaptureThreadResponse>,

    handle: JoinHandle<CaptureThread<P, DecoderEventCb, FormatChangedCb>>,
}
impl<OP, P, InputDoneCb, DecoderEventCb, FormatChangedCb> DecoderState
    for Decoding<OP, P, InputDoneCb, DecoderEventCb, FormatChangedCb>
where
    OP: BufferHandles,
    P: HandlesProvider,
    InputDoneCb: InputDoneCallback<OP>,
    DecoderEventCb: DecoderEventCallback<P>,
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
    #[error("Cannot drain now: output format not yet determined")]
    TryAgain,
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
type DequeueOutputBufferError<OP: BufferHandles> = ioctl::DqBufError<DqBuffer<Output, OP>>;
#[allow(type_alias_bounds)]
type CanceledBuffers<OP: BufferHandles> =
    Vec<<Queue<Output, BuffersAllocated<OP>> as Stream>::Canceled>;

impl<OP, P, InputDoneCb, DecoderEventCb, FormatChangedCb>
    Decoder<Decoding<OP, P, InputDoneCb, DecoderEventCb, FormatChangedCb>>
where
    OP: BufferHandles,
    P: HandlesProvider,
    InputDoneCb: InputDoneCallback<OP>,
    DecoderEventCb: DecoderEventCallback<P>,
    FormatChangedCb: FormatChangedCallback<P>,
{
    pub fn num_output_buffers(&self) -> usize {
        self.state.output_queue.num_buffers()
    }

    /// Send a command to the capture thread.
    fn send_command(&self, command: DecoderCommand) -> Result<(), SendCommandError> {
        trace!("Sending command: {:?}", command);

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
    /// processed, call the [`Decoder::drain`] method and wait for the output buffer with
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

    /// Drain the decoder, i.e. make sure all its pending work is processed.
    ///
    /// The `blocking` parameters decides whether this method is permitted to
    /// block: if `true`, then all the frames corresponding to the encoded
    /// buffers queued so far will have been emitted when this function returns.
    /// In this case the method will always return `true` to signal that drain
    /// has been completed.
    ///
    /// If `false`, then the method may also return `false` to signal that drain
    /// has not completed yet. When this is the case, the client must look for
    /// a decoded frame with the LAST flag set, as this flag signals that this
    /// frame is the last one before the drain has completed.
    ///
    /// Note that requesting a blocking drain can be hazardous if the current
    /// thread is responsible for e.g. submitting handles for decoded frames. It
    /// is easy to put the decoding pipeline in a deadlock situation.
    ///
    /// The client can keep submitting buffers with encoded data as the drain is
    /// ongoing. They will be processed in order and their frames will come
    /// after the ones still in the pipeline. For a way to cancel all the
    /// pending jobs, see the [`Decoder::flush`] method.
    pub fn drain(&self, blocking: bool) -> Result<bool, DrainError> {
        debug!("Drain requested");
        self.send_command(DecoderCommand::Drain(blocking))?;

        match self.state.response_receiver.recv()? {
            CaptureThreadResponse::DrainDone(response) => match response {
                Ok(completed) => Ok(completed),
                Err(e) => {
                    error!("Error while draining on the capture thread: {}", e);
                    Err(e)
                }
            },
            r => {
                error!(
                    "Unexpected capture thread response received while draining: {:?}",
                    r
                );
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
    /// If a [`Decoder::drain`] operation was in progress, it is also canceled.
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
            r => {
                error!(
                    "Unexpected capture thread response received while flushing: {:?}",
                    r
                );
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
                Err(ioctl::DqBufError::NotReady) => break,
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

impl<'a, OP, P, InputDoneCb, DecoderEventCb, FormatChangedCb> OutputQueueableProvider<'a, OP>
    for Decoder<Decoding<OP, P, InputDoneCb, DecoderEventCb, FormatChangedCb>>
where
    Queue<Output, BuffersAllocated<OP>>: OutputQueueableProvider<'a, OP>,
    OP: BufferHandles,
    P: HandlesProvider,
    InputDoneCb: InputDoneCallback<OP>,
    DecoderEventCb: DecoderEventCallback<P>,
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
impl<'a, OP, P, InputDoneCb, DecoderEventCb, FormatChangedCb>
    GetFreeOutputBuffer<'a, OP, GetBufferError<OP>>
    for Decoder<Decoding<OP, P, InputDoneCb, DecoderEventCb, FormatChangedCb>>
where
    Queue<Output, BuffersAllocated<OP>>: GetFreeOutputBuffer<'a, OP>,
    OP: BufferHandles,
    P: HandlesProvider,
    InputDoneCb: InputDoneCallback<OP>,
    DecoderEventCb: DecoderEventCallback<P>,
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
impl<'a, OP, P, InputDoneCb, DecoderEventCb, FormatChangedCb>
    Decoder<Decoding<OP, P, InputDoneCb, DecoderEventCb, FormatChangedCb>>
where
    Self: GetFreeOutputBuffer<'a, OP, GetBufferError<OP>>,
    OP: BufferHandles,
    P: HandlesProvider,
    InputDoneCb: InputDoneCallback<OP>,
    DecoderEventCb: DecoderEventCallback<P>,
    FormatChangedCb: FormatChangedCallback<P>,
{
    /// Returns the number of currently queued encoded buffers.
    pub fn num_queued_buffers(&self) -> usize {
        self.state.output_queue.num_queued_buffers()
    }

    /// Returns a V4L2 buffer to be filled with a frame to decode, waiting for
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
    pub fn kick(&self) -> Result<(), DequeueOutputBufferError<OP>> {
        info!("Kick!");
        self.dequeue_output_buffers()
    }
}

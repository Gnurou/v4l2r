use crate::{
    device::{
        poller::{DeviceEvent, PollEvents, Poller},
        queue::{
            direction::{Capture, Output},
            dqbuf::DQBuffer,
            dual_queue::{DualBufferHandles, DualQBuffer},
            qbuf::get_free::{GetFreeBuffer, GetFreeBufferError},
            qbuf::{Plane, QBuffer},
            BuffersAllocated, CanceledBuffer, CreateQueueError, FormatBuilder, Queue, QueueInit,
            RequestBuffersError,
        },
        AllocatedQueue, Device, DeviceConfig, DeviceOpenError, Stream, TryDequeue,
    },
    ioctl::{self, BufferFlags, DQBufError, EncoderCommand, FormatFlags, GFmtError},
    memory::{BufferHandles, HandlesProvider, PrimitiveBufferHandles},
    Format,
};

use std::{
    io,
    path::Path,
    sync::{atomic::AtomicUsize, Arc},
    thread::JoinHandle,
};
use thiserror::Error;

/// Trait implemented by all states of the encoder.
pub trait EncoderState {}

pub struct Encoder<S: EncoderState> {
    // Make sure to keep the device alive as long as we are.
    device: Arc<Device>,
    state: S,
}

pub struct AwaitingCaptureFormat {
    output_queue: Queue<Output, QueueInit>,
    capture_queue: Queue<Capture, QueueInit>,
}
impl EncoderState for AwaitingCaptureFormat {}

#[derive(Debug, Error)]
pub enum EncoderOpenError {
    #[error("Error while opening device")]
    DeviceOpenError(#[from] DeviceOpenError),
    #[error("Error while creating queue")]
    CreateQueueError(#[from] CreateQueueError),
    #[error("Specified device is not an encoder")]
    NotAnEncoder,
}

impl Encoder<AwaitingCaptureFormat> {
    pub fn open(path: &Path) -> Result<Self, EncoderOpenError> {
        let config = DeviceConfig::new().non_blocking_dqbuf();
        let device = Device::open(path, config)?;
        let device = Arc::new(device);

        // Check that the device is indeed an encoder.
        let capture_queue = Queue::get_capture_mplane_queue(device.clone())?;
        let output_queue = Queue::get_output_mplane_queue(device.clone())?;

        // On an encoder, the OUTPUT formats are not compressed, but the CAPTURE ones are.
        // Return an error if our device does not satisfy these conditions.
        output_queue
            .format_iter()
            .find(|fmt| !fmt.flags.contains(FormatFlags::COMPRESSED))
            .and(
                capture_queue
                    .format_iter()
                    .find(|fmt| fmt.flags.contains(FormatFlags::COMPRESSED)),
            )
            .ok_or(EncoderOpenError::NotAnEncoder)
            .map(|_| ())?;

        Ok(Encoder {
            device,
            state: AwaitingCaptureFormat {
                output_queue,
                capture_queue,
            },
        })
    }

    pub fn set_capture_format<F>(mut self, f: F) -> anyhow::Result<Encoder<AwaitingOutputFormat>>
    where
        F: FnOnce(FormatBuilder) -> anyhow::Result<()>,
    {
        let builder = self.state.capture_queue.change_format()?;
        f(builder)?;

        Ok(Encoder {
            device: self.device,
            state: AwaitingOutputFormat {
                output_queue: self.state.output_queue,
                capture_queue: self.state.capture_queue,
            },
        })
    }
}

pub struct AwaitingOutputFormat {
    output_queue: Queue<Output, QueueInit>,
    capture_queue: Queue<Capture, QueueInit>,
}
impl EncoderState for AwaitingOutputFormat {}

impl Encoder<AwaitingOutputFormat> {
    pub fn set_output_format<F>(mut self, f: F) -> anyhow::Result<Encoder<AwaitingOutputBuffers>>
    where
        F: FnOnce(FormatBuilder) -> anyhow::Result<()>,
    {
        let builder = self.state.output_queue.change_format()?;
        f(builder)?;

        Ok(Encoder {
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
impl EncoderState for AwaitingOutputBuffers {}

impl Encoder<AwaitingOutputBuffers> {
    pub fn allocate_output_buffers_generic<OP: BufferHandles>(
        self,
        memory_type: OP::SupportedMemoryType,
        num_output: usize,
    ) -> Result<Encoder<AwaitingCaptureBuffers<OP>>, RequestBuffersError> {
        Ok(Encoder {
            device: self.device,
            state: AwaitingCaptureBuffers {
                output_queue: self
                    .state
                    .output_queue
                    .request_buffers_generic::<OP>(memory_type, num_output as u32)?,
                capture_queue: self.state.capture_queue,
            },
        })
    }

    pub fn allocate_output_buffers<OP: PrimitiveBufferHandles>(
        self,
        num_output: usize,
    ) -> Result<Encoder<AwaitingCaptureBuffers<OP>>, RequestBuffersError> {
        self.allocate_output_buffers_generic(OP::MEMORY_TYPE, num_output)
    }

    pub fn get_output_format(&self) -> Result<Format, GFmtError> {
        self.state.output_queue.get_format()
    }

    pub fn get_capture_format(&self) -> Result<Format, GFmtError> {
        self.state.capture_queue.get_format()
    }
}

pub struct AwaitingCaptureBuffers<OP: BufferHandles> {
    output_queue: Queue<Output, BuffersAllocated<OP>>,
    capture_queue: Queue<Capture, QueueInit>,
}
impl<OP: BufferHandles> EncoderState for AwaitingCaptureBuffers<OP> {}

impl<OP: BufferHandles> Encoder<AwaitingCaptureBuffers<OP>> {
    pub fn allocate_capture_buffers<P: HandlesProvider>(
        self,
        num_capture: usize,
        capture_memory_provider: P,
    ) -> Result<Encoder<ReadyToEncode<OP, P>>, RequestBuffersError> {
        Ok(Encoder {
            device: self.device,
            state: ReadyToEncode {
                output_queue: self.state.output_queue,
                capture_queue: self
                    .state
                    .capture_queue
                    .request_buffers::<P::HandleType>(num_capture as u32)?,
                capture_memory_provider,
                poll_wakeups_counter: None,
            },
        })
    }
}

pub struct ReadyToEncode<OP: BufferHandles, P: HandlesProvider> {
    output_queue: Queue<Output, BuffersAllocated<OP>>,
    capture_queue: Queue<Capture, BuffersAllocated<P::HandleType>>,
    capture_memory_provider: P,
    poll_wakeups_counter: Option<Arc<AtomicUsize>>,
}
impl<OP: BufferHandles, P: HandlesProvider> EncoderState for ReadyToEncode<OP, P> {}

impl<OP: BufferHandles, P: HandlesProvider> Encoder<ReadyToEncode<OP, P>> {
    pub fn set_poll_counter(mut self, poll_wakeups_counter: Arc<AtomicUsize>) -> Self {
        self.state.poll_wakeups_counter = Some(poll_wakeups_counter);
        self
    }

    pub fn start<InputDoneCb, OutputReadyCb>(
        self,
        input_done_cb: InputDoneCb,
        output_ready_cb: OutputReadyCb,
    ) -> io::Result<Encoder<Encoding<OP, P, InputDoneCb, OutputReadyCb>>>
    where
        InputDoneCb: Fn(CompletedOutputBuffer<OP>),
        OutputReadyCb: FnMut(DQBuffer<Capture, P::HandleType>) + Send + 'static,
    {
        self.state.output_queue.stream_on().unwrap();
        self.state.capture_queue.stream_on().unwrap();

        let mut output_poller = Poller::new(Arc::clone(&self.device))?;
        output_poller.enable_event(DeviceEvent::OutputReady)?;

        let mut encoder_thread = EncoderThread::new(
            &self.device,
            self.state.capture_queue,
            self.state.capture_memory_provider,
            output_ready_cb,
        )?;

        if let Some(counter) = &self.state.poll_wakeups_counter {
            output_poller.set_poll_counter(Arc::clone(counter));
            encoder_thread.set_poll_counter(Arc::clone(counter));
        }

        let handle = std::thread::Builder::new()
            .name("V4L2 Encoder".into())
            .spawn(move || encoder_thread.run())?;

        Ok(Encoder {
            device: self.device,
            state: Encoding {
                output_queue: self.state.output_queue,
                input_done_cb,
                output_poller,
                handle,
            },
        })
    }
}

pub struct Encoding<OP: BufferHandles, P, InputDoneCb, OutputReadyCb>
where
    P: HandlesProvider,
    InputDoneCb: Fn(CompletedOutputBuffer<OP>),
    OutputReadyCb: FnMut(DQBuffer<Capture, P::HandleType>) + Send,
{
    output_queue: Queue<Output, BuffersAllocated<OP>>,
    input_done_cb: InputDoneCb,
    output_poller: Poller,

    handle: JoinHandle<EncoderThread<P, OutputReadyCb>>,
}
impl<OP, P, InputDoneCb, OutputReadyCb> EncoderState for Encoding<OP, P, InputDoneCb, OutputReadyCb>
where
    OP: BufferHandles,
    P: HandlesProvider,
    InputDoneCb: Fn(CompletedOutputBuffer<OP>),
    OutputReadyCb: FnMut(DQBuffer<Capture, P::HandleType>) + Send,
{
}

// Safe because all Rcs are internal and never leaked outside of the struct.
unsafe impl<S: EncoderState> Send for Encoder<S> {}

#[allow(type_alias_bounds)]
type DequeueOutputBufferError<OP: BufferHandles> = DQBufError<DQBuffer<Output, OP>>;

pub enum CompletedOutputBuffer<OP: BufferHandles> {
    Dequeued(DQBuffer<Output, OP>),
    Canceled(CanceledBuffer<OP>),
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

impl<OP, P, InputDoneCb, OutputReadyCb> Encoder<Encoding<OP, P, InputDoneCb, OutputReadyCb>>
where
    OP: BufferHandles,
    P: HandlesProvider,
    InputDoneCb: Fn(CompletedOutputBuffer<OP>),
    OutputReadyCb: FnMut(DQBuffer<Capture, P::HandleType>) + Send,
{
    /// Stop the encoder, and returns the encoder ready to be started again.
    pub fn stop(self) -> Result<Encoder<ReadyToEncode<OP, P>>, ()> {
        ioctl::encoder_cmd(&*self.device, EncoderCommand::Stop(false)).unwrap();

        // The encoder thread should receive the LAST buffer and exit on its own.
        let encoding_thread = self.state.handle.join().unwrap();

        encoding_thread.capture_queue.stream_off().unwrap();
        /* Return all canceled buffers to the client */
        let canceled_buffers = self.state.output_queue.stream_off().unwrap();
        for buffer in canceled_buffers {
            (self.state.input_done_cb)(CompletedOutputBuffer::Canceled(buffer));
        }

        Ok(Encoder {
            device: self.device,
            state: ReadyToEncode {
                output_queue: self.state.output_queue,
                capture_queue: encoding_thread.capture_queue,
                capture_memory_provider: encoding_thread.capture_memory_provider,
                poll_wakeups_counter: None,
            },
        })
    }

    /// Attempts to dequeue and release output buffers that the driver is done with.
    fn dequeue_output_buffers(&self) -> Result<(), DequeueOutputBufferError<OP>> {
        let output_queue = &self.state.output_queue;

        while output_queue.num_queued_buffers() > 0 {
            match output_queue.try_dequeue() {
                Ok(buf) => {
                    (self.state.input_done_cb)(CompletedOutputBuffer::Dequeued(buf));
                }
                Err(DQBufError::NotReady) => break,
                // TODO buffers with the error flag set should not result in
                // a fatal error!
                Err(e) => return Err(e),
            }
        }

        Ok(())
    }

    // Make this thread sleep until at least one OUTPUT buffer is ready to be
    // obtained through `try_get_buffer()`, dequeuing buffers if necessary.
    fn wait_for_output_buffer(&mut self) -> Result<(), GetBufferError<OP>> {
        for event in self.state.output_poller.poll(None)? {
            match event {
                PollEvents::DEVICE_OUTPUT => {
                    self.dequeue_output_buffers()?;
                }
                _ => panic!("Unexpected return from OUTPUT queue poll!"),
            }
        }

        Ok(())
    }
}

/// Support for primitive plane handles on the OUTPUT queue.
impl<'a, OP, P, InputDoneCb, OutputReadyCb> GetFreeBuffer<'a, GetBufferError<OP>>
    for Encoder<Encoding<OP, P, InputDoneCb, OutputReadyCb>>
where
    OP: PrimitiveBufferHandles,
    P: HandlesProvider,
    InputDoneCb: Fn(CompletedOutputBuffer<OP>),
    OutputReadyCb: FnMut(DQBuffer<Capture, P::HandleType>) + Send,
{
    type Queueable = QBuffer<'a, Output, OP, OP>;

    /// Returns a V4L2 buffer to be filled with a frame to encode if one
    /// is available.
    ///
    /// This method will return None immediately if all the allocated buffers
    /// are currently queued.
    fn try_get_free_buffer(&'a self) -> Result<Self::Queueable, GetBufferError<OP>> {
        self.dequeue_output_buffers()?;
        Ok(self.state.output_queue.try_get_free_buffer()?)
    }
}

/// Support for dynamic plane handles on the OUTPUT queue.
impl<'a, P, InputDoneCb, OutputReadyCb> GetFreeBuffer<'a, GetBufferError<DualBufferHandles>>
    for Encoder<Encoding<DualBufferHandles, P, InputDoneCb, OutputReadyCb>>
where
    P: HandlesProvider,
    InputDoneCb: Fn(CompletedOutputBuffer<DualBufferHandles>),
    OutputReadyCb: FnMut(DQBuffer<Capture, P::HandleType>) + Send,
{
    type Queueable = DualQBuffer<'a, Output>;

    /// Returns a V4L2 buffer to be filled with a frame to encode if one
    /// is available.
    ///
    /// This method will return None immediately if all the allocated buffers
    /// are currently queued.
    fn try_get_free_buffer(&'a self) -> Result<Self::Queueable, GetBufferError<DualBufferHandles>> {
        self.dequeue_output_buffers()?;
        Ok(self.state.output_queue.try_get_free_buffer()?)
    }
}

// If `GetFreeBuffer` is implemented, we can also provide a blocking `get_buffer`
// method.
impl<'a, OP, P, InputDoneCb, OutputReadyCb> Encoder<Encoding<OP, P, InputDoneCb, OutputReadyCb>>
where
    OP: BufferHandles,
    P: HandlesProvider,
    Self: GetFreeBuffer<'a, GetBufferError<OP>>,
    InputDoneCb: Fn(CompletedOutputBuffer<OP>),
    OutputReadyCb: FnMut(DQBuffer<Capture, P::HandleType>) + Send,
{
    /// Returns a V4L2 buffer to be filled with a frame to encode, waiting for
    /// one to be available if needed.
    ///
    /// Contrary to `try_get_free_buffer(), this method will wait for a buffer
    /// to be available if needed.
    pub fn get_buffer(
        &'a mut self,
    ) -> Result<<Self as GetFreeBuffer<'a, GetBufferError<OP>>>::Queueable, GetBufferError<OP>>
    {
        let output_queue = &self.state.output_queue;

        // If all our buffers are queued, wait until we can dequeue some.
        if output_queue.num_queued_buffers() == output_queue.num_buffers() {
            self.wait_for_output_buffer()?;
        }

        self.try_get_free_buffer()
    }
}

struct EncoderThread<P, OutputReadyCb>
where
    P: HandlesProvider,
    OutputReadyCb: FnMut(DQBuffer<Capture, P::HandleType>) + Send,
{
    capture_queue: Queue<Capture, BuffersAllocated<P::HandleType>>,
    capture_memory_provider: P,
    poller: Poller,
    output_ready_cb: OutputReadyCb,
}

impl<P, OutputReadyCb> EncoderThread<P, OutputReadyCb>
where
    P: HandlesProvider,
    OutputReadyCb: FnMut(DQBuffer<Capture, P::HandleType>) + Send,
{
    fn new(
        device: &Arc<Device>,
        capture_queue: Queue<Capture, BuffersAllocated<P::HandleType>>,
        capture_memory_provider: P,
        output_ready_cb: OutputReadyCb,
    ) -> io::Result<Self> {
        let mut poller = Poller::new(Arc::clone(device))?;

        // Mio only supports edge-triggered epoll, so it is important that we
        // register the V4L2 FD *before* queuing any capture buffers, so the
        // edge monitoring starts before any buffer can possibly be completed.
        poller.enable_event(DeviceEvent::CaptureReady)?;

        Ok(EncoderThread {
            capture_queue,
            capture_memory_provider,
            poller,
            output_ready_cb,
        })
    }

    fn set_poll_counter(&mut self, poll_wakeups_counter: Arc<AtomicUsize>) {
        self.poller.set_poll_counter(poll_wakeups_counter);
    }

    fn run(mut self) -> Self {
        self.enqueue_capture_buffers();

        'polling: loop {
            match self.capture_queue.num_queued_buffers() {
                // If there are no buffers on the CAPTURE queue, poll() will return
                // immediately with EPOLLERR and we would loop indefinitely.
                // Prevent this by temporarily disabling polling the device in such
                // cases.
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

            // TODO handle errors - this system call can be interrupted and we
            // should leave in this case.
            for event in self.poller.poll(None).unwrap() {
                match event {
                    // A CAPTURE buffer has been released by the client.
                    PollEvents::WAKER => {
                        // Requeue all available CAPTURE buffers.
                        self.enqueue_capture_buffers();
                    }
                    // A CAPTURE buffer is ready to be dequeued.
                    PollEvents::DEVICE_CAPTURE => {
                        // Get the encoded buffer
                        // TODO Manage errors here, including corrupted buffers!
                        if let Ok(mut cap_buf) = self.capture_queue.try_dequeue() {
                            let is_last = cap_buf.data.flags.contains(BufferFlags::LAST);
                            let is_empty = cap_buf.data.planes[0].bytesused == 0;

                            // Add a drop callback to the dequeued buffer so we
                            // re-queue it as soon as it is dropped.
                            let cap_waker = Arc::clone(self.poller.get_waker());
                            cap_buf.add_drop_callback(move |_dqbuf| {
                                // Intentionally ignore the result here.
                                let _ = cap_waker.wake();
                            });

                            // Empty buffers do not need to be passed to the client.
                            if !is_empty {
                                (self.output_ready_cb)(cap_buf);
                            }

                            // Last buffer of the stream? Time for us to terminate.
                            if is_last {
                                break 'polling;
                            }
                        } else {
                            // TODO we should not crash here.
                            panic!("Expected a CAPTURE buffer but none available!");
                        }
                    }
                    _ => panic!("Unexpected return from CAPTURE queue poll!"),
                }
            }
        }

        self
    }

    fn enqueue_capture_buffers(&mut self) {
        while let Ok(mut buffer) = self.capture_queue.try_get_free_buffer() {
            if let Some(handles) = self.capture_memory_provider.get_handles() {
                while buffer.num_set_planes() < buffer.num_expected_planes() {
                    buffer = buffer.add_plane(Plane::cap());
                }
                buffer.queue_with_handles(handles).unwrap();
            }
        }
    }
}

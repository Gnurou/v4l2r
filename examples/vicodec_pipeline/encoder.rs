use v4l2::device::{
    queue::{
        direction, dqbuf,
        qbuf::get_free::{GetFreeBuffer, GetFreeBufferError},
        qbuf::QBuffer,
        BuffersAllocated, CreateQueueError, FormatBuilder, Queue, QueueInit, RequestBuffersError,
    },
    AllocatedQueue,
};
use v4l2::device::{Device, DeviceConfig, DeviceOpenError, Stream, TryDequeue};
use v4l2::ioctl::{BufferFlags, DQBufError, EncoderCommand, FormatFlags, GFmtError};
use v4l2::memory::{UserPtr, MMAP};

use mio::{self, unix::SourceFd, Events, Interest, Poll, Token, Waker};
use std::os::unix::io::AsRawFd;
use std::path::Path;
use std::{
    io,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    thread::JoinHandle,
};

use direction::{Capture, Output};
use dqbuf::DQBuffer;
use thiserror::Error;

/// Trait implemented by all states of the encoder.
pub trait EncoderState {}

pub struct Encoder<S: EncoderState> {
    // Make sure to keep the device alive as long as we are.
    device: Arc<Device>,
    state: S,
}

pub struct AwaitingCaptureFormat {
    output_queue: Queue<direction::Output, QueueInit>,
    capture_queue: Queue<direction::Capture, QueueInit>,
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

    pub fn set_capture_format(
        mut self,
        f: fn(FormatBuilder) -> anyhow::Result<()>,
    ) -> anyhow::Result<Encoder<AwaitingOutputFormat>> {
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
    output_queue: Queue<direction::Output, QueueInit>,
    capture_queue: Queue<direction::Capture, QueueInit>,
}
impl EncoderState for AwaitingOutputFormat {}

impl Encoder<AwaitingOutputFormat> {
    pub fn set_output_format(
        mut self,
        f: fn(FormatBuilder) -> anyhow::Result<()>,
    ) -> anyhow::Result<Encoder<AwaitingOutputBuffers>> {
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
    output_queue: Queue<direction::Output, QueueInit>,
    capture_queue: Queue<direction::Capture, QueueInit>,
}
impl EncoderState for AwaitingOutputBuffers {}

impl Encoder<AwaitingOutputBuffers> {
    pub fn allocate_output_buffers(
        self,
        num_output: usize,
    ) -> Result<Encoder<AwaitingCaptureBuffers>, RequestBuffersError> {
        Ok(Encoder {
            device: self.device,
            state: AwaitingCaptureBuffers {
                output_queue: self
                    .state
                    .output_queue
                    .request_buffers::<UserPtr<_>>(num_output as u32)?,
                capture_queue: self.state.capture_queue,
            },
        })
    }

    pub fn get_output_format(&self) -> Result<v4l2::Format, GFmtError> {
        self.state.output_queue.get_format()
    }

    pub fn get_capture_format(&self) -> Result<v4l2::Format, GFmtError> {
        self.state.capture_queue.get_format()
    }
}

pub struct AwaitingCaptureBuffers {
    output_queue: Queue<direction::Output, BuffersAllocated<UserPtr<Vec<u8>>>>,
    capture_queue: Queue<direction::Capture, QueueInit>,
}
impl EncoderState for AwaitingCaptureBuffers {}

impl Encoder<AwaitingCaptureBuffers> {
    pub fn allocate_capture_buffers(
        self,
        num_capture: usize,
    ) -> Result<Encoder<ReadyToEncode>, RequestBuffersError> {
        Ok(Encoder {
            device: self.device,
            state: ReadyToEncode {
                output_queue: self.state.output_queue,
                capture_queue: self
                    .state
                    .capture_queue
                    .request_buffers::<MMAP>(num_capture as u32)?,
                poll_wakeups_counter: None,
            },
        })
    }
}

const CAPTURE_READY: Token = Token(1);
const OUTPUT_READY: Token = Token(2);
/// Waker for all self-triggered events. Can only use one per Poll.
const WAKER: Token = Token(1000);

pub struct ReadyToEncode {
    output_queue: Queue<direction::Output, BuffersAllocated<UserPtr<Vec<u8>>>>,
    capture_queue: Queue<direction::Capture, BuffersAllocated<MMAP>>,
    poll_wakeups_counter: Option<Arc<AtomicUsize>>,
}
impl EncoderState for ReadyToEncode {}

impl Encoder<ReadyToEncode> {
    pub fn set_poll_counter(mut self, poll_wakeups_counter: Arc<AtomicUsize>) -> Self {
        self.state.poll_wakeups_counter = Some(poll_wakeups_counter);
        self
    }

    pub fn start<InputDoneCb, OutputReadyCb>(
        self,
        input_done_cb: InputDoneCb,
        output_ready_cb: OutputReadyCb,
    ) -> Result<Encoder<Encoding<InputDoneCb, OutputReadyCb>>, io::Error>
    where
        InputDoneCb: Fn(&mut Vec<Vec<u8>>),
        OutputReadyCb: FnMut(DQBuffer<Capture, MMAP>) + Send + 'static,
    {
        self.state.output_queue.stream_on().unwrap();
        self.state.capture_queue.stream_on().unwrap();

        let encoder_thread =
            EncoderThread::new(&self.device, self.state.capture_queue, output_ready_cb)?;
        let encoder_thread = if let Some(counter) = &self.state.poll_wakeups_counter {
            encoder_thread.set_poll_counter(Arc::clone(counter))
        } else {
            encoder_thread
        };

        let output_poll = Poll::new()?;
        output_poll.registry().register(
            &mut SourceFd(&self.device.as_raw_fd()),
            OUTPUT_READY,
            Interest::WRITABLE,
        )?;

        let handle = std::thread::Builder::new()
            .name("V4L2 Encoder".into())
            .spawn(move || encoder_thread.run())?;

        Ok(Encoder {
            device: self.device,
            state: Encoding {
                output_queue: self.state.output_queue,
                input_done_cb,
                output_poll,
                handle,
                num_poll_wakeups: self.state.poll_wakeups_counter,
            },
        })
    }
}

pub struct Encoding<InputDoneCb, OutputReadyCb>
where
    InputDoneCb: Fn(&mut Vec<Vec<u8>>),
    OutputReadyCb: FnMut(DQBuffer<Capture, MMAP>) + Send,
{
    output_queue: Queue<direction::Output, BuffersAllocated<UserPtr<Vec<u8>>>>,
    input_done_cb: InputDoneCb,
    output_poll: Poll,

    handle: JoinHandle<EncoderThread<OutputReadyCb>>,
    // Number of times we have awaken from a poll, for stats purposes.
    num_poll_wakeups: Option<Arc<AtomicUsize>>,
}
impl<InputDoneCb, OutputReadyCb> EncoderState for Encoding<InputDoneCb, OutputReadyCb>
where
    InputDoneCb: Fn(&mut Vec<Vec<u8>>),
    OutputReadyCb: FnMut(DQBuffer<Capture, MMAP>) + Send,
{
}

// Safe because all Rcs are internal and never leaked outside of the struct.
unsafe impl<S: EncoderState> Send for Encoder<S> {}

type OutputBuffer<'a> = QBuffer<'a, Output, UserPtr<Vec<u8>>>;
type DequeueOutputBufferError = DQBufError<DQBuffer<Output, UserPtr<Vec<u8>>>>;

#[derive(Debug, Error)]
pub enum GetBufferError {
    #[error("Error while dequeueing buffer")]
    DequeueError(#[from] DequeueOutputBufferError),
    #[error("Error during poll")]
    PollError(#[from] io::Error),
    #[error("Error while obtaining buffer")]
    GetFreeBufferError(#[from] GetFreeBufferError),
}

impl<InputDoneCb, OutputReadyCb> Encoder<Encoding<InputDoneCb, OutputReadyCb>>
where
    InputDoneCb: Fn(&mut Vec<Vec<u8>>),
    OutputReadyCb: FnMut(DQBuffer<Capture, MMAP>) + Send,
{
    /// Stop the encoder, and returns the encoder ready to be started again.
    pub fn stop(self) -> Result<Encoder<ReadyToEncode>, ()> {
        v4l2::ioctl::encoder_cmd(&*self.device, EncoderCommand::Stop(false)).unwrap();

        // The encoder thread should receive the LAST buffer and exit on its own.
        let encoding_thread = self.state.handle.join().unwrap();

        encoding_thread.capture_queue.stream_off().unwrap();
        /* Return all canceled buffers to the client */
        let canceled_buffers = self.state.output_queue.stream_off().unwrap();
        for mut buffer in canceled_buffers {
            (self.state.input_done_cb)(&mut buffer.plane_handles);
        }

        Ok(Encoder {
            device: self.device,
            state: ReadyToEncode {
                output_queue: self.state.output_queue,
                capture_queue: encoding_thread.capture_queue,
                poll_wakeups_counter: None,
            },
        })
    }

    /// Attempts to dequeue and release output buffers that the driver is done with.
    fn dequeue_output_buffers(&self) -> Result<(), DequeueOutputBufferError> {
        let output_queue = &self.state.output_queue;

        while output_queue.num_queued_buffers() > 0 {
            match output_queue.try_dequeue() {
                Ok(mut buf) => {
                    (self.state.input_done_cb)(&mut buf.plane_handles);
                }
                Err(DQBufError::NotReady) => break,
                Err(e) => return Err(e),
            }
        }

        Ok(())
    }

    // Make this thread sleep until at least one OUTPUT buffer is ready to be
    // obtained through `try_get_buffer()`, dequeuing buffers if necessary.
    fn wait_for_output_buffer(&mut self) -> Result<(), GetBufferError> {
        let mut events = Events::with_capacity(1);
        // TODO use timeout!
        self.state.output_poll.poll(&mut events, None)?;
        if let Some(poll_counter) = &self.state.num_poll_wakeups {
            poll_counter.fetch_add(1, Ordering::SeqCst);
        }
        for event in &events {
            match event.token() {
                OUTPUT_READY => {
                    if event.is_writable() {
                        // We call dequeue_output_buffers() here to make sure
                        // that all the buffers signaled by poll() are dequeued,
                        // since Mio only supports edge-triggered polling and
                        // leaving buffers unattended may create a deadlock the
                        // next time this method is called.
                        self.dequeue_output_buffers()?;
                    } else if event.is_error() {
                        // This can happen if we enter here while no buffer
                        // is queued and is ok.
                    } else {
                        unreachable!();
                    }
                }
                // We are only polling for one kind of event, so should never
                // reach here.
                _ => unreachable!(),
            }
        }

        Ok(())
    }

    /// Returns a V4L2 buffer to be filled with a frame to encode if one
    /// is available.
    ///
    /// This method will return None immediately if all the allocated buffers
    /// are currently queued.
    #[allow(dead_code)]
    pub fn try_get_buffer(&self) -> Result<OutputBuffer, GetBufferError> {
        self.dequeue_output_buffers()?;
        Ok(self.state.output_queue.try_get_free_buffer()?)
    }

    /// Returns a V4L2 buffer to be filled with a frame to encode, waiting for
    /// one to be available if needed.
    ///
    /// If all allocated buffers are currently queued, this methods will wait
    /// for one to be available.
    pub fn get_buffer(&mut self) -> Result<OutputBuffer, GetBufferError> {
        let output_queue = &self.state.output_queue;

        self.dequeue_output_buffers()?;
        // If all our buffers are queued, wait until we can dequeue some.
        if output_queue.num_queued_buffers() == output_queue.num_buffers() {
            self.wait_for_output_buffer()?;
        }

        Ok(self.state.output_queue.try_get_free_buffer()?)
    }
}

struct EncoderThread<OutputReadyCb>
where
    OutputReadyCb: FnMut(DQBuffer<Capture, MMAP>) + Send,
{
    device: Arc<Device>,
    capture_queue: Queue<direction::Capture, BuffersAllocated<MMAP>>,
    poll: Poll,
    waker: Arc<Waker>,
    output_ready_cb: OutputReadyCb,
    // Number of times we have awaken from a poll, for stats purposes.
    poll_wakeups_counter: Option<Arc<AtomicUsize>>,
}

impl<OutputReadyCb> EncoderThread<OutputReadyCb>
where
    OutputReadyCb: FnMut(DQBuffer<Capture, MMAP>) + Send,
{
    fn new(
        device: &Arc<Device>,
        capture_queue: Queue<Capture, BuffersAllocated<MMAP>>,
        output_ready_cb: OutputReadyCb,
    ) -> io::Result<Self> {
        let poll = Poll::new()?;
        let waker = Arc::new(Waker::new(poll.registry(), WAKER)?);

        let encoder_thread = EncoderThread {
            device: Arc::clone(&device),
            capture_queue,
            poll,
            waker,
            output_ready_cb,
            poll_wakeups_counter: None,
        };

        // Mio only supports edge-triggered epoll, so it is important that we
        // register the V4L2 FD *before* queuing any capture buffers, so the
        // edge monitoring starts before any buffer can possibly be completed.
        encoder_thread.enable_device_polling()?;

        Ok(encoder_thread)
    }

    fn set_poll_counter(mut self, poll_wakeups_counter: Arc<AtomicUsize>) -> Self {
        self.poll_wakeups_counter = Some(poll_wakeups_counter);
        self
    }

    fn enable_device_polling(&self) -> io::Result<()> {
        self.poll.registry().register(
            &mut SourceFd(&self.device.as_raw_fd()),
            CAPTURE_READY,
            Interest::READABLE,
        )
    }

    fn disable_device_polling(&self) -> io::Result<()> {
        self.poll
            .registry()
            .deregister(&mut SourceFd(&self.device.as_raw_fd()))
    }

    fn run(mut self) -> Self {
        let mut polling_device = true;
        let mut events = Events::with_capacity(4);
        self.enqueue_capture_buffers();

        'polling: loop {
            // If there are no buffers on the CAPTURE queue, poll() will return
            // immediately with EPOLLERR and we would loop indefinitely.
            // Prevent this by temporarily disabling polling the device in such
            // cases.
            if polling_device && self.capture_queue.num_queued_buffers() == 0 {
                self.disable_device_polling().unwrap();
                polling_device = false;
            }
            self.poll.poll(&mut events, None).unwrap();
            if let Some(poll_counter) = &self.poll_wakeups_counter {
                poll_counter.fetch_add(1, Ordering::SeqCst);
            }
            for event in &events {
                if event.is_error() {
                    // This happens if we try to poll while no CAPTURE buffer
                    // is queued. We try to avoid that, so reaching this block
                    // would be a bug.
                    unreachable!()
                }

                match event.token() {
                    WAKER => {
                        // If device polling was disabled, we can reenable it as
                        // we are queuing a new buffer to the capture queue.
                        // This must be done BEFORE queueing so the edge changes
                        // after we registered the device.
                        if !polling_device {
                            self.enable_device_polling().unwrap();
                            polling_device = true;
                        }
                        // A CAPTURE buffer has been released, requeue it.
                        self.enqueue_capture_buffers();
                    }
                    CAPTURE_READY => {
                        if event.is_readable() {
                            // Get the encoded buffer
                            // Mio only supports edge-triggered polling, so make
                            // sure to dequeue all the buffers that were available
                            // when poll() returned, as they won't be signaled a
                            // second time.
                            // TODO Manage errors here, including corrupted buffers!
                            while let Ok(mut cap_buf) = self.capture_queue.try_dequeue() {
                                let is_last = cap_buf.data.flags.contains(BufferFlags::LAST);
                                let bytes_used = cap_buf.data.planes[0].bytesused;

                                // Zero-size buffers can be ignored.
                                if bytes_used > 0 {
                                    // Add a drop callback to the dequeued buffer so
                                    // we re-queue it as soon as it is dropped.
                                    let cap_waker = self.waker.clone();
                                    cap_buf.add_drop_callback(move |_dqbuf| {
                                        // Intentionally ignore the result here.
                                        let _ = cap_waker.wake();
                                    });
                                    (self.output_ready_cb)(cap_buf);
                                }

                                // Last buffer of the stream? Time for us to terminate.
                                if is_last {
                                    break 'polling;
                                }
                            }
                        } else if event.is_priority() {
                            todo!("V4L2 events not implemented yet");
                        }
                    }
                    _ => unreachable!(),
                }
            }
        }

        self
    }

    fn enqueue_capture_buffers(&mut self) {
        while let Ok(buffer) = self.capture_queue.try_get_free_buffer() {
            buffer.auto_queue().unwrap();
        }
    }
}

use v4l2::device::queue::{direction, dqbuf, states, FormatBuilder, Queue};
use v4l2::device::{Device, DeviceConfig};
use v4l2::ioctl::{DQBufError, FormatFlags};
use v4l2::memory::{UserPtr, MMAP};

use mio::{self, unix::SourceFd, Events, Interest, Poll, Token, Waker};
use std::os::unix::io::AsRawFd;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, channel, Receiver, Sender};
use std::{fmt, sync::Arc, thread::JoinHandle};

use direction::Capture;
use mpsc::{RecvError, TryIter};
use thiserror::Error;

#[derive(Debug, PartialEq)]
enum Command {
    Stop,
    EncodeFrame(Vec<u8>),
}

pub enum Message {
    InputBufferDone(Vec<u8>),
    FrameEncoded(dqbuf::DQBuffer<Capture, MMAP>),
}

/// Trait implemented by all states of the encoder.
pub trait EncoderState {}

pub struct AwaitingCaptureFormat {
    output_queue: Queue<direction::Output, states::QueueInit>,
    capture_queue: Queue<direction::Capture, states::QueueInit>,
}
impl EncoderState for AwaitingCaptureFormat {}

pub struct AwaitingOutputFormat {
    output_queue: Queue<direction::Output, states::QueueInit>,
    capture_queue: Queue<direction::Capture, states::QueueInit>,
}
impl EncoderState for AwaitingOutputFormat {}

pub struct AwaitingBufferAllocation {
    output_queue: Queue<direction::Output, states::QueueInit>,
    capture_queue: Queue<direction::Capture, states::QueueInit>,
}
impl EncoderState for AwaitingBufferAllocation {}

pub struct ReadyToEncode {
    output_queue: Queue<direction::Output, states::BuffersAllocated<UserPtr<Vec<u8>>>>,
    capture_queue: Queue<direction::Capture, states::BuffersAllocated<MMAP>>,
    // The number of encoding jobs currently in progress, i.e. the number of
    // OUTPUT buffers we are currently using. The client increases it before
    // submitting a job, and the encode decreases it when a job completes.
    jobs_in_progress: Arc<AtomicUsize>,
    // Number of times we have awaken from a poll, for stats purposes.
    num_poll_wakeups: Arc<AtomicUsize>,
}
impl EncoderState for ReadyToEncode {}

pub struct Encoding {
    handle: JoinHandle<EncoderThread>,
    send: Sender<Command>,
    /// Inform the encoder thread that we have sent a message through `send`.
    waker: Arc<Waker>,
    pub recv: Receiver<Message>,
    jobs_in_progress: Arc<AtomicUsize>,
    /// The client will start rejecting new encode jobs if the number of encode
    /// requests goes beyond this number. Set to the number of output buffers.
    max_jobs: usize,
    num_poll_wakeups: Arc<AtomicUsize>,
}
impl EncoderState for Encoding {}

#[derive(Error)]
pub enum EncodeError {
    #[error("queue is currently full")]
    QueueFull(Vec<u8>),
    #[error("error sending command")]
    SendError(#[from] SendError),
}

// Without this custom implementation the whole input buffer would be dumped
// in the debug print.
impl fmt::Debug for EncodeError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            EncodeError::QueueFull(_) => write!(f, "EncodeError::QueueFull"),
            EncodeError::SendError(e) => write!(f, "EncodeError::SendError: {:?}", e),
        }
    }
}

pub type EncodeResult<T> = std::result::Result<T, EncodeError>;

#[derive(Debug, Error)]
pub enum StopError {
    #[error("error sending stop command: {0}")]
    SendError(#[from] SendError),
    #[error("thread not responding")]
    ThreadBlocked,
}
pub type StopResult<T> = std::result::Result<T, StopError>;

#[derive(Debug, Error)]
pub enum SendError {
    #[error("channel send error")]
    ChannelSendError,
    #[error("io error: {0}")]
    IoError(std::io::Error),
}
pub type SendResult<T> = std::result::Result<T, SendError>;

pub struct Encoder<S: EncoderState> {
    // Make sure to keep the device alive as long as we are.
    device: Arc<Device>,
    state: S,
}

// Safe because all Rcs are internal and never leaked outside of the struct.
unsafe impl<S: EncoderState> Send for Encoder<S> {}

#[derive(Debug, Error)]
enum ProcessError {
    #[error("V4L2 error: {0}")]
    V4L2Error(#[from] v4l2::Error),
    #[error("send error")]
    SendError,
}

impl<T> From<mpsc::SendError<T>> for ProcessError {
    fn from(_e: mpsc::SendError<T>) -> Self {
        ProcessError::SendError
    }
}

type ProcessResult<T> = std::result::Result<T, ProcessError>;

impl Encoder<AwaitingCaptureFormat> {
    pub fn open(path: &Path) -> v4l2::Result<Self> {
        let config = DeviceConfig::new().non_blocking_dqbuf();
        let device = Device::open(path, config)?;
        let device = Arc::new(device);

        // Check that the device is indeed an encoder.
        let capture_queue = Queue::get_capture_mplane_queue(device.clone())?;
        let output_queue = Queue::get_output_mplane_queue(device.clone())?;

        // On an encoder, the OUTPUT formats are not compressed...
        if output_queue
            .format_iter()
            .find(|fmt| !fmt.flags.contains(FormatFlags::COMPRESSED))
            .is_none()
        {
            panic!("This is not an encoder: input formats are not raw.");
        }

        // But the CAPTURE ones are.
        if capture_queue
            .format_iter()
            .find(|fmt| fmt.flags.contains(FormatFlags::COMPRESSED))
            .is_none()
        {
            panic!("This is not an encoder: output formats are not compressed.");
        }

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

impl Encoder<AwaitingOutputFormat> {
    pub fn set_output_format(
        mut self,
        f: fn(FormatBuilder) -> anyhow::Result<()>,
    ) -> anyhow::Result<Encoder<AwaitingBufferAllocation>> {
        let builder = self.state.output_queue.change_format()?;
        f(builder)?;

        Ok(Encoder {
            device: self.device,
            state: AwaitingBufferAllocation {
                output_queue: self.state.output_queue,
                capture_queue: self.state.capture_queue,
            },
        })
    }
}

impl Encoder<AwaitingBufferAllocation> {
    pub fn allocate_buffers(
        self,
        num_output: usize,
        num_capture: usize,
    ) -> v4l2::Result<Encoder<ReadyToEncode>> {
        let output_queue = self
            .state
            .output_queue
            .request_buffers::<UserPtr<_>>(num_output as u32)?;
        let capture_queue = self
            .state
            .capture_queue
            .request_buffers::<MMAP>(num_capture as u32)?;

        Ok(Encoder {
            device: self.device,
            state: ReadyToEncode {
                output_queue,
                capture_queue,
                jobs_in_progress: Arc::new(AtomicUsize::new(0)),
                num_poll_wakeups: Arc::new(AtomicUsize::new(0)),
            },
        })
    }

    pub fn get_output_format(&self) -> v4l2::Result<v4l2::Format> {
        self.state.output_queue.get_format()
    }

    pub fn get_capture_format(&self) -> v4l2::Result<v4l2::Format> {
        self.state.capture_queue.get_format()
    }
}

impl Encoder<ReadyToEncode> {
    pub fn start_encoding(self) -> v4l2::Result<Encoder<Encoding>> {
        let (cmd_send, cmd_recv) = channel();
        let (msg_send, msg_recv) = channel();

        let poll = Poll::new().unwrap();
        let waker = Arc::new(Waker::new(poll.registry(), WAKER).unwrap());
        let thread_waker = waker.clone();

        let max_jobs = self.state.output_queue.num_buffers();

        self.state.output_queue.streamon().unwrap();
        self.state.capture_queue.streamon().unwrap();

        let encoder_thread = EncoderThread {
            device: self.device.clone(),
            output_queue: self.state.output_queue,
            capture_queue: self.state.capture_queue,
            jobs_in_progress: self.state.jobs_in_progress.clone(),
            num_poll_wakeups: self.state.num_poll_wakeups.clone(),
        };

        let handle = std::thread::Builder::new()
            .name("V4L2 Encoder".into())
            .spawn(move || encoder_thread.run(cmd_recv, msg_send, poll, thread_waker))
            .unwrap();

        Ok(Encoder {
            device: self.device,
            state: Encoding {
                handle,
                send: cmd_send,
                waker,
                recv: msg_recv,
                jobs_in_progress: self.state.jobs_in_progress,
                max_jobs,
                num_poll_wakeups: self.state.num_poll_wakeups,
            },
        })
    }
}

impl Encoder<Encoding> {
    fn send(&self, command: Command) -> SendResult<()> {
        if self.state.send.send(command).is_err() {
            return Err(SendError::ChannelSendError);
        }
        if let Err(e) = self.state.waker.wake() {
            return Err(SendError::IoError(e));
        }

        Ok(())
    }

    /// Stop the encoder, and return the thread handle we can wait on if we are
    /// interested in getting it back.
    pub fn stop(self) -> StopResult<Encoder<ReadyToEncode>> {
        self.send(Command::Stop)?;

        // Wait for the thread to close the connection, to indicate it has
        // acknowledged the stop command.
        loop {
            match self
                .state
                .recv
                .recv_timeout(std::time::Duration::from_millis(1000))
            {
                Err(mpsc::RecvTimeoutError::Timeout) => return Err(StopError::ThreadBlocked),
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Ok(_) => {
                    // TODO get buffers that were pending, as well as those that
                    // were streamed off and return then to the client.
                }
            }
        }

        let encoding_thread = self.state.handle.join().unwrap();

        encoding_thread.capture_queue.streamoff().unwrap();
        encoding_thread.output_queue.streamoff().unwrap();

        Ok(Encoder {
            device: self.device,
            state: ReadyToEncode {
                output_queue: encoding_thread.output_queue,
                capture_queue: encoding_thread.capture_queue,
                jobs_in_progress: self.state.jobs_in_progress,
                num_poll_wakeups: encoding_thread.num_poll_wakeups,
            },
        })
    }

    pub fn encode(&self, frame: Vec<u8>) -> EncodeResult<()> {
        let num_jobs = self.state.jobs_in_progress.fetch_add(1, Ordering::SeqCst);
        if num_jobs >= self.state.max_jobs {
            self.state.jobs_in_progress.fetch_sub(1, Ordering::SeqCst);
            return Err(EncodeError::QueueFull(frame));
        }

        self.send(Command::EncodeFrame(frame))?;

        Ok(())
    }

    pub fn get_num_poll_wakeups(&self) -> usize {
        self.state.num_poll_wakeups.load(Ordering::SeqCst)
    }

    pub fn recv(&self) -> core::result::Result<Message, RecvError> {
        self.state.recv.recv()
    }

    pub fn try_iter(&self) -> TryIter<Message> {
        self.state.recv.try_iter()
    }
}

struct EncoderThread {
    device: Arc<Device>,
    output_queue: Queue<direction::Output, states::BuffersAllocated<UserPtr<Vec<u8>>>>,
    capture_queue: Queue<direction::Capture, states::BuffersAllocated<MMAP>>,
    // The number of encoding jobs currently in progress, i.e. the number of
    // OUTPUT buffers we are currently using. The client increases it before
    // submitting a job, and the encode decreases it when a job completes.
    jobs_in_progress: Arc<AtomicUsize>,
    // Number of times we have awaken from a poll, for stats purposes.
    num_poll_wakeups: Arc<AtomicUsize>,
}

const WAKER: Token = Token(1000);

impl EncoderThread {
    fn run(
        mut self,
        cmd_recv: Receiver<Command>,
        msg_send: Sender<Message>,
        mut poll: Poll,
        waker: Arc<Waker>,
    ) -> Self {
        const DRIVER: Token = Token(1);

        let mut events = Events::with_capacity(4);
        let device_fd = self.device.as_raw_fd();
        let mut interest = Interest::READABLE;
        poll.registry()
            .register(&mut SourceFd(&device_fd), DRIVER, interest)
            .unwrap();

        self.enqueue_capture_buffers();

        'poll_loop: loop {
            // Stop polling on writable if not all output buffers are queued, or
            // start polling on writable if all output buffers are queued.
            if interest.is_writable()
                && self.output_queue.num_queued_buffers() < self.output_queue.num_buffers()
            {
                interest = Interest::READABLE;
                poll.registry()
                    .reregister(&mut SourceFd(&device_fd), DRIVER, interest)
                    .unwrap();
            } else if !interest.is_writable()
                && self.output_queue.num_queued_buffers() >= self.output_queue.num_buffers()
            {
                interest = Interest::READABLE | Interest::WRITABLE;
                poll.registry()
                    .reregister(&mut SourceFd(&device_fd), DRIVER, interest)
                    .unwrap();
            };

            poll.poll(&mut events, None).unwrap();
            self.num_poll_wakeups.fetch_add(1, Ordering::SeqCst);
            for event in &events {
                match event.token() {
                    WAKER => {
                        // First possible source: we received a new command.
                        while let Ok(cmd) = cmd_recv.try_recv() {
                            match self.process_command(cmd, &msg_send) {
                                Ok(true) => (),
                                Ok(false) => {
                                    drop(msg_send);
                                    break 'poll_loop;
                                }
                                Err(e) => panic!("Platform error: {}", e),
                            }
                        }

                        // Second possible source: a capture buffer has been released.
                        self.enqueue_capture_buffers();
                    }
                    DRIVER => {
                        if event.is_priority() {
                            todo!("V4L2 events not implemented yet");
                        }

                        if event.is_readable() {
                            // Get the encoded buffer
                            if let Ok(mut cap_buf) = self.capture_queue.dequeue() {
                                let cap_waker = waker.clone();
                                cap_buf.set_drop_callback(move |_dqbuf| {
                                    // Intentionally ignore the result here.
                                    let _ = cap_waker.wake();
                                });
                                msg_send.send(Message::FrameEncoded(cap_buf)).unwrap();
                            }
                            self.try_dequeue_output_buffers(&msg_send);
                        }

                        if event.is_writable() {
                            self.try_dequeue_output_buffers(&msg_send);
                        }
                    }
                    _ => unreachable!(),
                }
            }
        }

        self
    }

    fn enqueue_capture_buffers(&mut self) {
        while let Some(buffer) = self.capture_queue.get_free_buffer() {
            buffer.auto_queue().unwrap();
        }
    }

    fn enqueue_output_buffer(&mut self, buffer_data: Vec<u8>) {
        let buffer = self.output_queue.get_free_buffer().unwrap();
        let bytes_used = buffer_data.len();
        buffer.add_plane(buffer_data, bytes_used).queue().unwrap();
    }

    fn try_dequeue_output_buffers(&mut self, msg_send: &Sender<Message>) {
        loop {
            match self.output_queue.dequeue() {
                Ok(mut out_buf) => {
                    let handles = out_buf.plane_handles.remove(0);
                    drop(out_buf);
                    self.jobs_in_progress.fetch_sub(1, Ordering::SeqCst);
                    msg_send.send(Message::InputBufferDone(handles)).unwrap();
                }
                Err(DQBufError::NotReady) => break,
                _ => panic!("Unrecoverable error"),
            }
        }
    }

    fn process_command(&mut self, cmd: Command, msg_send: &Sender<Message>) -> ProcessResult<bool> {
        match cmd {
            Command::Stop => {
                // Stop the CAPTURE queue and lose all buffers.
                self.capture_queue.streamoff()?;

                // Stop the OUTPUT queue and return all handles to client.
                let canceled_buffers = self.output_queue.streamoff()?;
                for mut buffer in canceled_buffers {
                    msg_send.send(Message::InputBufferDone(buffer.plane_handles.remove(0)))?;
                }
                return Ok(false);
            }
            Command::EncodeFrame(frame) => {
                self.try_dequeue_output_buffers(msg_send);
                self.enqueue_output_buffer(frame);
            }
        }

        Ok(true)
    }
}

use v4l2::device::queue::{direction, dqbuf, qbuf::QBuffer, states, FormatBuilder, Queue};
use v4l2::device::{Device, DeviceConfig};
use v4l2::ioctl::{BufferFlags, DQBufError, FormatFlags};
use v4l2::memory::{UserPtr, MMAP};

use mio::{self, unix::SourceFd, Events, Interest, Poll, Token, Waker};
use std::os::unix::io::AsRawFd;
use std::path::Path;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Barrier,
    },
    thread::JoinHandle,
};

use direction::{Capture, Output};
use dqbuf::DQBuffer;
use thiserror::Error;

enum Command {
    /// Ask the encoder thread to also poll the availability of buffers to
    /// dequeue on the OUTPUT queue, and to run the callback when one
    /// becomes available. Once the callback is run, the encoder thread will
    /// stop monitoring the OUTPUT queue until this command is sent again.
    WatchOutputQueue(Box<dyn FnOnce() + Send>),
    Stop,
}

/// Trait implemented by all states of the encoder.
pub trait EncoderState {}

pub struct Encoder<S: EncoderState> {
    // Make sure to keep the device alive as long as we are.
    device: Arc<Device>,
    state: S,
}

pub struct AwaitingCaptureFormat {
    output_queue: Queue<direction::Output, states::QueueInit>,
    capture_queue: Queue<direction::Capture, states::QueueInit>,
}
impl EncoderState for AwaitingCaptureFormat {}

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

pub struct AwaitingOutputFormat {
    output_queue: Queue<direction::Output, states::QueueInit>,
    capture_queue: Queue<direction::Capture, states::QueueInit>,
}
impl EncoderState for AwaitingOutputFormat {}

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

pub struct AwaitingBufferAllocation {
    output_queue: Queue<direction::Output, states::QueueInit>,
    capture_queue: Queue<direction::Capture, states::QueueInit>,
}
impl EncoderState for AwaitingBufferAllocation {}

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

pub struct ReadyToEncode {
    output_queue: Queue<direction::Output, states::BuffersAllocated<UserPtr<Vec<u8>>>>,
    capture_queue: Queue<direction::Capture, states::BuffersAllocated<MMAP>>,
}
impl EncoderState for ReadyToEncode {}

impl Encoder<ReadyToEncode> {
    pub fn start_encoding<InputDoneCb, OutputReadyCb>(
        self,
        input_done_cb: InputDoneCb,
        output_ready_cb: OutputReadyCb,
    ) -> v4l2::Result<Encoder<Encoding<InputDoneCb, OutputReadyCb>>>
    where
        InputDoneCb: Fn(&mut Vec<Vec<u8>>),
        OutputReadyCb: FnMut(DQBuffer<Capture, MMAP>) + Send + 'static,
    {
        let (cmd_send, cmd_recv) = channel();

        let poll = Poll::new().unwrap();
        let waker = Arc::new(Waker::new(poll.registry(), WAKER).unwrap());
        let thread_waker = waker.clone();

        self.state.output_queue.streamon().unwrap();
        self.state.capture_queue.streamon().unwrap();

        let encoder_thread = EncoderThread {
            device: self.device.clone(),
            capture_queue: self.state.capture_queue,
            num_poll_wakeups: Arc::new(AtomicUsize::new(0)),
            watch_output_cb: None,
            output_ready_cb,
        };

        let handle = std::thread::Builder::new()
            .name("V4L2 Encoder".into())
            .spawn(move || encoder_thread.run(cmd_recv, poll, thread_waker))
            .unwrap();

        Ok(Encoder {
            device: self.device,
            state: Encoding {
                output_queue: self.state.output_queue,
                input_done_cb,
                handle,
                send: cmd_send,
                waker,
            },
        })
    }
}

pub struct Encoding<InputDoneCb, OutputReadyCb>
where
    InputDoneCb: Fn(&mut Vec<Vec<u8>>),
    OutputReadyCb: FnMut(DQBuffer<Capture, MMAP>) + Send,
{
    output_queue: Queue<direction::Output, states::BuffersAllocated<UserPtr<Vec<u8>>>>,
    input_done_cb: InputDoneCb,

    handle: JoinHandle<EncoderThread<OutputReadyCb>>,
    send: Sender<Command>,
    /// Inform the encoder thread that we have sent a message through `send`.
    waker: Arc<Waker>,
}
impl<InputDoneCb, OutputReadyCb> EncoderState for Encoding<InputDoneCb, OutputReadyCb>
where
    InputDoneCb: Fn(&mut Vec<Vec<u8>>),
    OutputReadyCb: FnMut(DQBuffer<Capture, MMAP>) + Send,
{
}

#[derive(Debug, Error)]
pub enum StopError {
    #[error("error sending stop command: {0}")]
    SendError(#[from] SendError),
}

#[derive(Debug, Error)]
pub enum SendError {
    #[error("channel send error")]
    ChannelSendError,
    #[error("io error: {0}")]
    IoError(std::io::Error),
}

// Safe because all Rcs are internal and never leaked outside of the struct.
unsafe impl<S: EncoderState> Send for Encoder<S> {}

impl<InputDoneCb, OutputReadyCb> Encoder<Encoding<InputDoneCb, OutputReadyCb>>
where
    InputDoneCb: Fn(&mut Vec<Vec<u8>>),
    OutputReadyCb: FnMut(DQBuffer<Capture, MMAP>) + Send,
{
    fn send(&self, command: Command) -> Result<(), SendError> {
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
    pub fn stop(self) -> Result<Encoder<ReadyToEncode>, StopError> {
        self.send(Command::Stop)?;

        let encoding_thread = self.state.handle.join().unwrap();

        encoding_thread.capture_queue.streamoff().unwrap();
        self.state.output_queue.streamoff().unwrap();

        Ok(Encoder {
            device: self.device,
            state: ReadyToEncode {
                output_queue: self.state.output_queue,
                capture_queue: encoding_thread.capture_queue,
            },
        })
    }

    fn dequeue_output_buffers(&self) {
        let output_queue = &self.state.output_queue;

        // If have more than half of our buffers queued, attempt to dequeue some.
        while output_queue.num_queued_buffers() >= (output_queue.num_buffers() + 1) / 2 {
            match output_queue.dequeue() {
                Ok(mut buf) => {
                    (self.state.input_done_cb)(&mut buf.plane_handles);
                }
                Err(DQBufError::NotReady) => break,
                // TODO this should return a Result<>.
                Err(e) => panic!(e),
            }
        }
    }

    /// Returns a V4L2 buffer to be filled with a frame to encode if one
    /// is available.
    ///
    /// This method will return None immediately if all the allocated buffers
    /// are currently queued.
    ///
    /// TODO return a Result<> with proper errors.
    pub fn try_get_buffer(&self) -> Option<QBuffer<Output, UserPtr<Vec<u8>>>> {
        self.dequeue_output_buffers();
        self.state.output_queue.get_free_buffer()
    }

    /// Returns a V4L2 buffer to be filled with a frame to encode, waiting for
    /// one to be available if needed.
    ///
    /// If all allocated buffers are currently queued, this methods will wait
    /// for one to be available.
    pub fn get_buffer(&self) -> Option<QBuffer<Output, UserPtr<Vec<u8>>>> {
        if let Some(buffer) = self.try_get_buffer() {
            Some(buffer)
        } else {
            let barrier = Arc::new(Barrier::new(2));
            let barrier_poll = barrier.clone();
            self.send(Command::WatchOutputQueue(Box::new(move || {
                barrier_poll.wait();
            })))
            .unwrap();
            // This may make the encoder thread block if it processes our request first,
            // which is unlikely but not optimal if it happens.
            barrier.wait();
            // Now we should definitely be able to dequeue at least one input buffer.
            self.try_get_buffer()
        }
    }
}

struct EncoderThread<OutputReadyCb>
where
    OutputReadyCb: FnMut(DQBuffer<Capture, MMAP>) + Send,
{
    device: Arc<Device>,
    capture_queue: Queue<direction::Capture, states::BuffersAllocated<MMAP>>,
    watch_output_cb: Option<Box<dyn FnOnce() + Send>>,
    output_ready_cb: OutputReadyCb,
    // Number of times we have awaken from a poll, for stats purposes.
    num_poll_wakeups: Arc<AtomicUsize>,
}

const WAKER: Token = Token(1000);

impl<OutputReadyCb> EncoderThread<OutputReadyCb>
where
    OutputReadyCb: FnMut(DQBuffer<Capture, MMAP>) + Send,
{
    fn run(mut self, cmd_recv: Receiver<Command>, mut poll: Poll, waker: Arc<Waker>) -> Self {
        const DRIVER: Token = Token(1);

        let mut events = Events::with_capacity(4);
        let device_fd = self.device.as_raw_fd();
        poll.registry()
            .register(&mut SourceFd(&device_fd), DRIVER, Interest::READABLE)
            .unwrap();

        self.enqueue_capture_buffers();

        'poll_loop: loop {
            poll.poll(&mut events, None).unwrap();
            self.num_poll_wakeups.fetch_add(1, Ordering::SeqCst);
            for event in &events {
                match event.token() {
                    WAKER => {
                        // First possible source: we received a new command.
                        // TODO move into own method?
                        while let Ok(cmd) = cmd_recv.try_recv() {
                            match cmd {
                                Command::WatchOutputQueue(cb) => {
                                    self.watch_output_cb = Some(cb);
                                    poll.registry()
                                        .reregister(
                                            &mut SourceFd(&device_fd),
                                            DRIVER,
                                            Interest::READABLE | Interest::WRITABLE,
                                        )
                                        .unwrap();
                                }
                                Command::Stop => {
                                    // Stop the CAPTURE queue and lose all buffers.
                                    // TODO handle errors properly.
                                    self.capture_queue.streamoff().unwrap();

                                    // Stop the OUTPUT queue and return all handles to client.
                                    /*
                                    // TODO we need to move this into the client thread so it
                                    // retrieves all the handles.
                                    let canceled_buffers = self.output_queue.streamoff()?;
                                    for mut buffer in canceled_buffers {
                                        msg_send.send(Message::InputBufferDone(buffer.plane_handles.remove(0)))?;
                                    }
                                    */
                                    break 'poll_loop;
                                }
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
                                let is_last = cap_buf.data.flags.contains(BufferFlags::LAST);

                                let cap_waker = waker.clone();
                                cap_buf.set_drop_callback(move |_dqbuf| {
                                    // Intentionally ignore the result here.
                                    let _ = cap_waker.wake();
                                });
                                (self.output_ready_cb)(cap_buf);

                                if is_last {
                                    break 'poll_loop;
                                }
                            } else {
                                eprintln!("Poll awaken but no capture buffer available. This is a driver bug.");
                            }
                        }

                        if event.is_writable() {
                            // If we are here we must have a closure ready to call. Otherwise that's
                            // and error.
                            let closure = self
                                .watch_output_cb
                                .take()
                                .expect("Output buffer ready signaled but no closure to call");
                            closure();

                            // Signal the client thread that it can now get an OUTPUT buffer,
                            // and go back to our business.
                            poll.registry()
                                .reregister(&mut SourceFd(&device_fd), DRIVER, Interest::READABLE)
                                .unwrap();
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
}

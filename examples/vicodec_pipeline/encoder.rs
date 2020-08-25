use v4l2::device::queue::{
    direction, dqbuf, qbuf::QBuffer, states, CreateQueueError, FormatBuilder, Queue,
    RequestBuffersError,
};
use v4l2::device::{Device, DeviceConfig, DeviceOpenError};
use v4l2::ioctl::{BufferFlags, DQBufError, EncoderCommand, FormatFlags, GFmtError};
use v4l2::memory::{UserPtr, MMAP};

use mio::{self, unix::SourceFd, Events, Interest, Poll, Token, Waker};
use std::os::unix::io::AsRawFd;
use std::path::Path;
use std::{
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
    output_queue: Queue<direction::Output, states::QueueInit>,
    capture_queue: Queue<direction::Capture, states::QueueInit>,
}
impl EncoderState for AwaitingCaptureFormat {}

#[derive(Debug, Error)]
pub enum EncoderOpenError {
    #[error("Error while opening device")]
    DeviceOpenError(#[from] DeviceOpenError),
    #[error("Error while creating queue")]
    CreateQueueError(#[from] CreateQueueError),
}

impl Encoder<AwaitingCaptureFormat> {
    pub fn open(path: &Path) -> Result<Self, EncoderOpenError> {
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
    ) -> Result<Encoder<ReadyToEncode>, RequestBuffersError> {
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

    pub fn get_output_format(&self) -> Result<v4l2::Format, GFmtError> {
        self.state.output_queue.get_format()
    }

    pub fn get_capture_format(&self) -> Result<v4l2::Format, GFmtError> {
        self.state.capture_queue.get_format()
    }
}

const CAPTURE_READY: Token = Token(1);
const OUTPUT_READY: Token = Token(2);
/// Waker for all self-triggered events. Can only use one per Poll.
const WAKER: Token = Token(1000);

pub struct ReadyToEncode {
    output_queue: Queue<direction::Output, states::BuffersAllocated<UserPtr<Vec<u8>>>>,
    capture_queue: Queue<direction::Capture, states::BuffersAllocated<MMAP>>,
}
impl EncoderState for ReadyToEncode {}

impl Encoder<ReadyToEncode> {
    pub fn start<InputDoneCb, OutputReadyCb>(
        self,
        input_done_cb: InputDoneCb,
        output_ready_cb: OutputReadyCb,
    ) -> v4l2::Result<Encoder<Encoding<InputDoneCb, OutputReadyCb>>>
    where
        InputDoneCb: Fn(&mut Vec<Vec<u8>>),
        OutputReadyCb: FnMut(DQBuffer<Capture, MMAP>) + Send + 'static,
    {
        let poll = Poll::new().unwrap();
        let waker = Arc::new(Waker::new(poll.registry(), WAKER).unwrap());

        // Mio only supports edge-triggered epoll, so it is important that we
        // register the V4L2 FD *before* queuing any capture buffers, so the
        // edge monitoring starts before any buffer can possibly be completed.
        poll.registry()
            .register(
                &mut SourceFd(&self.device.as_raw_fd()),
                CAPTURE_READY,
                Interest::READABLE,
            )
            .unwrap();

        self.state.output_queue.streamon().unwrap();
        self.state.capture_queue.streamon().unwrap();

        let encoder_thread = EncoderThread {
            device: Arc::clone(&self.device),
            capture_queue: self.state.capture_queue,
            num_poll_wakeups: Arc::new(AtomicUsize::new(0)),
            output_ready_cb,
        };

        let output_poll = Poll::new().unwrap();
        output_poll
            .registry()
            .register(
                &mut SourceFd(&self.device.as_raw_fd()),
                OUTPUT_READY,
                Interest::WRITABLE,
            )
            .unwrap();

        let handle = std::thread::Builder::new()
            .name("V4L2 Encoder".into())
            .spawn(move || encoder_thread.run(poll, waker))
            .unwrap();

        Ok(Encoder {
            device: self.device,
            state: Encoding {
                output_queue: self.state.output_queue,
                input_done_cb,
                output_poll,
                handle,
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

    output_poll: Poll,

    handle: JoinHandle<EncoderThread<OutputReadyCb>>,
}
impl<InputDoneCb, OutputReadyCb> EncoderState for Encoding<InputDoneCb, OutputReadyCb>
where
    InputDoneCb: Fn(&mut Vec<Vec<u8>>),
    OutputReadyCb: FnMut(DQBuffer<Capture, MMAP>) + Send,
{
}

// Safe because all Rcs are internal and never leaked outside of the struct.
unsafe impl<S: EncoderState> Send for Encoder<S> {}

impl<InputDoneCb, OutputReadyCb> Encoder<Encoding<InputDoneCb, OutputReadyCb>>
where
    InputDoneCb: Fn(&mut Vec<Vec<u8>>),
    OutputReadyCb: FnMut(DQBuffer<Capture, MMAP>) + Send,
{
    /// Stop the encoder, and return the thread handle we can wait on if we are
    /// interested in getting it back.
    pub fn stop(self) -> Result<Encoder<ReadyToEncode>, ()> {
        v4l2::ioctl::encoder_cmd(&*self.device, EncoderCommand::Stop(false)).unwrap();

        // The encoder thread should receive the LAST buffer and exit on its own.
        let encoding_thread = self.state.handle.join().unwrap();

        encoding_thread.capture_queue.streamoff().unwrap();
        // TODO retrieve handles back and call the input done callback on them.
        self.state.output_queue.streamoff().unwrap();

        Ok(Encoder {
            device: self.device,
            state: ReadyToEncode {
                output_queue: self.state.output_queue,
                capture_queue: encoding_thread.capture_queue,
            },
        })
    }

    /// Attempts to dequeue and release output buffers that the driver is done with.
    fn dequeue_output_buffers(&self) {
        let output_queue = &self.state.output_queue;

        while output_queue.num_queued_buffers() > 0 {
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

    // Make this thread sleep until at least one OUTPUT buffer is ready to be
    // dequeued, and dequeues all available buffers.
    fn wait_for_output_buffer(&mut self) {
        let mut events = Events::with_capacity(1);
        // TODO use timeout!
        self.state.output_poll.poll(&mut events, None).unwrap();
        for event in &events {
            match event.token() {
                OUTPUT_READY => {
                    if event.is_writable() {
                        // We call dequeue_output_buffers() here to make sure
                        // that all the buffers signaled by poll() are dequeued,
                        // since Mio only supports edge-triggered polling and
                        // leaving buffers unattended may create a deadlock the
                        // next time this method is called.
                        self.dequeue_output_buffers();
                        return;
                    } else if event.is_error() {
                        // This can happen if we enter here while no buffer
                        // is queued and is ok.
                        return;
                    } else {
                        unreachable!();
                    }
                }
                _ => unreachable!(),
            }
        }
        unreachable!();
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
    pub fn get_buffer(&mut self) -> Option<QBuffer<Output, UserPtr<Vec<u8>>>> {
        if self.try_get_buffer().is_none() {
            self.wait_for_output_buffer();
        }

        self.state.output_queue.get_free_buffer()
    }
}

struct EncoderThread<OutputReadyCb>
where
    OutputReadyCb: FnMut(DQBuffer<Capture, MMAP>) + Send,
{
    device: Arc<Device>,
    capture_queue: Queue<direction::Capture, states::BuffersAllocated<MMAP>>,
    output_ready_cb: OutputReadyCb,
    // Number of times we have awaken from a poll, for stats purposes.
    num_poll_wakeups: Arc<AtomicUsize>,
}

impl<OutputReadyCb> EncoderThread<OutputReadyCb>
where
    OutputReadyCb: FnMut(DQBuffer<Capture, MMAP>) + Send,
{
    fn run(mut self, mut poll: Poll, waker: Arc<Waker>) -> Self {
        let mut polling_device = true;
        let mut events = Events::with_capacity(4);
        self.enqueue_capture_buffers();

        'poll_loop: loop {
            // If there are no buffers on the CAPTURE queue, poll() will return
            // immediately with EPOLLERR and we would loop indefinitely.
            // Prevent this by temporarily disabling polling the device in such
            // cases.
            if polling_device && self.capture_queue.num_queued_buffers() == 0 {
                poll.registry()
                    .deregister(&mut SourceFd(&self.device.as_raw_fd()))
                    .unwrap();
                polling_device = false;
            }
            poll.poll(&mut events, None).unwrap();
            self.num_poll_wakeups.fetch_add(1, Ordering::SeqCst);
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
                            poll.registry()
                                .register(
                                    &mut SourceFd(&self.device.as_raw_fd()),
                                    CAPTURE_READY,
                                    Interest::READABLE,
                                )
                                .unwrap();
                            polling_device = true;
                        }
                        // A CAPTURE buffer has been released, requeue it.
                        self.enqueue_capture_buffers();
                    }
                    CAPTURE_READY => {
                        if event.is_priority() {
                            todo!("V4L2 events not implemented yet");
                        }

                        if event.is_readable() {
                            // Get the encoded buffer
                            // Mio only supports edge-triggered polling, so make
                            // sure to dequeue all the buffers that were available
                            // when poll() returned, as they won't be signaled a
                            // second time.
                            // TODO Manage errors here, including corrupted buffers!
                            while let Ok(mut cap_buf) = self.capture_queue.dequeue() {
                                let is_last = cap_buf.data.flags.contains(BufferFlags::LAST);
                                let bytes_used = cap_buf.data.planes[0].bytesused;

                                // Zero-size buffers can be ignored.
                                if bytes_used > 0 {
                                    // Add a drop callback to the dequeued buffer so
                                    // we re-queue it as soon as it is dropped.
                                    let cap_waker = waker.clone();
                                    cap_buf.set_drop_callback(move |_dqbuf| {
                                        // Intentionally ignore the result here.
                                        let _ = cap_waker.wake();
                                    });
                                    (self.output_ready_cb)(cap_buf);
                                }

                                // Last buffer of the stream? Time for us to terminate.
                                if is_last {
                                    break 'poll_loop;
                                }
                            }
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

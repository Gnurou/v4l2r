use super::{Command, Encoder, Message, ReadyToEncode};

use mio::Waker;
use std::fmt;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::thread::JoinHandle;

pub struct Client {
    pub(super) handle: JoinHandle<Encoder<ReadyToEncode>>,
    pub(super) send: Sender<Command>,
    /// Inform the encoder thread that we have sent a message through `send`.
    pub(super) waker: Arc<Waker>,
    pub recv: Receiver<Message>,
    pub(super) jobs_in_progress: Arc<AtomicUsize>,
    /// The client will start rejecting new encode jobs if the number of encode
    /// requests goes beyond this number. Set to the number of output buffers.
    pub(super) max_jobs: usize,
    pub num_poll_wakeups: Arc<AtomicUsize>,
}

pub enum EncodeError {
    QueueFull(Vec<u8>),
    SendError(SendError),
}

impl fmt::Display for EncodeError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            EncodeError::QueueFull(_) => write!(f, "Queue is currently full"),
            EncodeError::SendError(e) => e.fmt(f),
        }
    }
}

impl fmt::Debug for EncodeError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            EncodeError::QueueFull(_) => write!(f, "EncodeError::QueueFull"),
            EncodeError::SendError(_) => write!(f, "EncodeError::SendError"),
        }
    }
}

impl From<SendError> for EncodeError {
    fn from(e: SendError) -> Self {
        EncodeError::SendError(e)
    }
}

impl std::error::Error for EncodeError {}

type EncodeResult<T> = std::result::Result<T, EncodeError>;

#[derive(Debug)]
pub enum StopError {
    SendError(SendError),
    ThreadBlocked,
}

impl fmt::Display for StopError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            StopError::SendError(e) => e.fmt(f),
            StopError::ThreadBlocked => write!(f, "Thread not responding"),
        }
    }
}

impl From<SendError> for StopError {
    fn from(e: SendError) -> Self {
        StopError::SendError(e)
    }
}

impl std::error::Error for StopError {}

type StopResult<T> = std::result::Result<T, StopError>;

#[derive(Debug)]
pub enum SendError {
    ChannelSendError,
    IoError(std::io::Error),
}

impl fmt::Display for SendError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            SendError::ChannelSendError => write!(f, "Channel send error"),
            SendError::IoError(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for SendError {}
type SendResult<T> = std::result::Result<T, SendError>;

impl Client {
    fn send(&self, command: Command) -> SendResult<()> {
        if let Err(_) = self.send.send(command) {
            return Err(SendError::ChannelSendError);
        }
        if let Err(e) = self.waker.wake() {
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
                .recv
                .recv_timeout(std::time::Duration::from_millis(1000))
            {
                Err(mpsc::RecvTimeoutError::Timeout) => return Err(StopError::ThreadBlocked),
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Ok(_) => {
                    // TODO get buffers that were pending, as well as those that
                    // were streamed off.
                }
            }
        }

        Ok(self.handle.join().unwrap())
    }

    pub fn encode(&self, frame: Vec<u8>) -> EncodeResult<()> {
        let num_jobs = self.jobs_in_progress.fetch_add(1, Ordering::SeqCst);
        if num_jobs >= self.max_jobs {
            self.jobs_in_progress.fetch_sub(1, Ordering::SeqCst);
            return Err(EncodeError::QueueFull(frame));
        }

        self.send(Command::EncodeFrame(frame))?;

        Ok(())
    }
}

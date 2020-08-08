use super::{Command, Encoder, Message, ReadyToEncode};

use mio::Waker;
use std::fmt;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::thread::JoinHandle;
use thiserror::Error;

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

impl Client {
    fn send(&self, command: Command) -> SendResult<()> {
        if self.send.send(command).is_err() {
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
                    // were streamed off and return then to the client.
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

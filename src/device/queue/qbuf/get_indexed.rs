//! Traits for trying to obtain a queueable, writable buffer from its index.
//!
//! `try_get_buffer()` returns the buffer with specified `index`, provided that
//! this buffer is currently available for use.
//!
//! The returned buffer shall not outlive the object that produced it.

use thiserror::Error;

use crate::memory::BufferHandles;

use super::{CaptureQueueable, OutputQueueable};

#[derive(Debug, Error)]
pub enum TryGetBufferError {
    #[error("Buffer with provided index {0} does not exist")]
    InvalidIndex(usize),
    #[error("Buffer is already in use")]
    AlreadyUsed,
}

pub trait GetOutputBufferByIndex<'a, P: BufferHandles> {
    type Queueable: 'a + OutputQueueable<P>;

    fn try_get_buffer(&'a self, index: usize) -> Result<Self::Queueable, TryGetBufferError>;
}

pub trait GetCaptureBufferByIndex<'a, P: BufferHandles> {
    type Queueable: 'a + CaptureQueueable<P>;

    fn try_get_buffer(&'a self, index: usize) -> Result<Self::Queueable, TryGetBufferError>;
}

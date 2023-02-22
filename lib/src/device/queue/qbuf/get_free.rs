//! Traits for buffers providers with their own allocation policy. Users of this
//! interface leave the choice of which buffer to return to the implementor,
//! which must define its own allocation policy.
//!
//! The returned buffer shall not outlive the object that produced it.

use thiserror::Error;

use crate::memory::BufferHandles;

use super::{CaptureQueueableProvider, OutputQueueableProvider};

#[derive(Debug, Error)]
pub enum GetFreeBufferError {
    #[error("all buffers are currently being used")]
    NoFreeBuffer,
}

pub trait GetFreeOutputBuffer<'a, P: BufferHandles, ErrorType = GetFreeBufferError>
where
    Self: OutputQueueableProvider<'a, P>,
{
    fn try_get_free_buffer(&'a self) -> Result<Self::Queueable, ErrorType>;
}

pub trait GetFreeCaptureBuffer<'a, P: BufferHandles, ErrorType = GetFreeBufferError>
where
    Self: CaptureQueueableProvider<'a, P>,
{
    fn try_get_free_buffer(&'a self) -> Result<Self::Queueable, ErrorType>;
}

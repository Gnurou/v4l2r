use thiserror::Error;

#[derive(Debug, Error)]
pub enum TryGetBufferError {
    #[error("Buffer with provided index {0} does not exist")]
    InvalidIndex(usize),
    #[error("Buffer is already in use")]
    AlreadyUsed,
}

/// A trait for trying to obtain a queueable, writable buffer from its index.
///
/// Returns the buffer with specified `index`, provided that this buffer is
/// currently available for use.
pub trait GetBufferByIndex<'a> {
    type Queueable;

    fn try_get_buffer(&'a self, index: usize) -> Result<Self::Queueable, TryGetBufferError>;
}

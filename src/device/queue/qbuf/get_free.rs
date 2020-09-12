use thiserror::Error;

#[derive(Debug, Error)]
pub enum GetFreeBufferError {
    #[error("All buffers are currently being used")]
    NoFreeBuffer,
}

/// Trait for buffers providers with their own allocation policy. Users of this
/// interface leave the choice of which buffer to return to the implementor,
/// which must define its own allocation policy.
pub trait GetFreeBuffer<'a> {
    type Queueable;

    fn try_get_free_buffer(&'a self) -> Result<Self::Queueable, GetFreeBufferError>;
}

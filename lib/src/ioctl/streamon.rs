//! Safe wrapper for the `VIDIOC_STREAM(ON|OFF)` ioctls.
use crate::QueueType;
use nix::errno::Errno;
use nix::Error;
use std::os::unix::io::AsRawFd;
use thiserror::Error;

#[doc(hidden)]
mod ioctl {
    nix::ioctl_write_ptr!(vidioc_streamon, b'V', 18, u32);
    nix::ioctl_write_ptr!(vidioc_streamoff, b'V', 19, u32);
}

#[derive(Debug, Error)]
pub enum StreamOnError {
    #[error("queue type ({0}) not supported, or no buffers allocated or enqueued")]
    InvalidQueue(QueueType),
    #[error("invalid pad configuration")]
    InvalidPadConfig,
    #[error("invalid pipeline link configuration")]
    InvalidPipelineConfig,
    #[error("ioctl error: {0}")]
    IoctlError(Error),
}

/// Safe wrapper around the `VIDIOC_STREAMON` ioctl.
pub fn streamon(fd: &impl AsRawFd, queue: QueueType) -> Result<(), StreamOnError> {
    match unsafe { ioctl::vidioc_streamon(fd.as_raw_fd(), &(queue as u32)) } {
        Ok(_) => Ok(()),
        Err(Errno::EINVAL) => Err(StreamOnError::InvalidQueue(queue)),
        Err(Errno::EPIPE) => Err(StreamOnError::InvalidPadConfig),
        Err(Errno::ENOLINK) => Err(StreamOnError::InvalidPipelineConfig),
        Err(e) => Err(StreamOnError::IoctlError(e)),
    }
}

#[derive(Debug, Error)]
pub enum StreamOffError {
    #[error("queue type not supported")]
    InvalidQueue,
    #[error("ioctl error: {0}")]
    IoctlError(Error),
}

/// Safe wrapper around the `VIDIOC_STREAMOFF` ioctl.
pub fn streamoff(fd: &impl AsRawFd, queue: QueueType) -> Result<(), StreamOffError> {
    match unsafe { ioctl::vidioc_streamoff(fd.as_raw_fd(), &(queue as u32)) } {
        Ok(_) => Ok(()),
        Err(Errno::EINVAL) => Err(StreamOffError::InvalidQueue),
        Err(e) => Err(StreamOffError::IoctlError(e)),
    }
}

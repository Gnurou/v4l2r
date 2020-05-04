//! Safe wrapper for the `VIDIOC_STREAM(ON|OFF)` ioctls.
use crate::QueueType;
use crate::Result;
use std::os::unix::io::AsRawFd;

#[doc(hidden)]
mod ioctl {
    nix::ioctl_write_ptr!(vidioc_streamon, b'V', 18, u32);
    nix::ioctl_write_ptr!(vidioc_streamoff, b'V', 19, u32);
}

/// Safe wrapper around the `VIDIOC_STREAMON` ioctl.
pub fn streamon(fd: &mut impl AsRawFd, queue: QueueType) -> Result<()> {
    unsafe { ioctl::vidioc_streamon(fd.as_raw_fd(), &(queue as u32)) }?;

    Ok(())
}

/// Safe wrapper around the `VIDIOC_STREAMOFF` ioctl.
pub fn streamoff(fd: &mut impl AsRawFd, queue: QueueType) -> Result<()> {
    unsafe { ioctl::vidioc_streamoff(fd.as_raw_fd(), &(queue as u32)) }?;

    Ok(())
}

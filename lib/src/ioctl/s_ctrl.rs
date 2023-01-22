use std::convert::From;
use std::os::unix::io::AsRawFd;

use nix::{self, errno::Errno, Error};
use thiserror::Error;

use crate::bindings;

/// Implementors can receive the result from the `s_ctrl` ioctl.
pub trait Ctrl: Sized {
    fn from_v4l2_control(v4l2_control: bindings::v4l2_control) -> Self;
}

/// Information for a V4L2 control. Safe variant of `struct v4l2_control`.
#[derive(Debug, Clone)]
pub struct Control {
    v4l2_control: bindings::v4l2_control,
}

impl Control {
    pub fn id(&self) -> u32 {
        self.v4l2_control.id
    }

    pub fn value(&self) -> i32 {
        self.v4l2_control.value
    }
}

impl Ctrl for Control {
    fn from_v4l2_control(v4l2_control: bindings::v4l2_control) -> Self {
        Self { v4l2_control }
    }
}

#[derive(Debug, Error)]
pub enum SCtrlError {
    #[error("Invalid v4l2_control ID")]
    InvalidId,
    #[error("The v4l2_control valueis out of bounds")]
    ValueOutOfBounds,
    #[error("Device currently busy")]
    DeviceBusy,
    #[error("Attempted to set a read only control")]
    ReadOnly,
    #[error("Unexpected ioctl error: {0}")]
    IoctlError(nix::Error),
}

impl From<Error> for SCtrlError {
    fn from(error: Error) -> Self {
        match error {
            Errno::EINVAL => Self::InvalidId,
            Errno::ERANGE => Self::ValueOutOfBounds,
            Errno::EBUSY => Self::DeviceBusy,
            Errno::EACCES => Self::ReadOnly,
            error => Self::IoctlError(error),
        }
    }
}

pub type SCtrlResult<T> = Result<T, SCtrlError>;

#[doc(hidden)]
mod ioctl {
    use crate::bindings::v4l2_control;
    nix::ioctl_readwrite!(vidioc_s_ctrl, b'V', 28, v4l2_control);
}

/// Safe wrapper around the `VIDIOC_S_CTRL` ioctl.
pub fn s_ctrl<T: Ctrl + std::fmt::Debug, F: AsRawFd>(
    fd: &mut F,
    id: u32,
    value: i32,
) -> Result<T, SCtrlError> {
    let mut ctrl = bindings::v4l2_control { id, value };
    unsafe { ioctl::vidioc_s_ctrl(fd.as_raw_fd(), &mut ctrl)? };
    Ok(T::from_v4l2_control(ctrl))
}

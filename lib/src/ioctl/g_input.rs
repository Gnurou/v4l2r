use std::ffi::c_int;
use std::os::unix::io::AsRawFd;

use nix::errno::Errno;
use thiserror::Error;

use crate::bindings::v4l2_input;
use crate::bindings::v4l2_output;

#[doc(hidden)]
mod ioctl {
    use std::ffi::c_int;

    use crate::bindings::v4l2_input;
    use crate::bindings::v4l2_output;

    nix::ioctl_readwrite!(vidioc_enuminput, b'V', 26, v4l2_input);
    nix::ioctl_read!(vidioc_g_input, b'V', 38, c_int);
    nix::ioctl_readwrite!(vidioc_s_input, b'V', 39, c_int);

    nix::ioctl_read!(vidioc_g_output, b'V', 46, c_int);
    nix::ioctl_readwrite!(vidioc_s_output, b'V', 47, c_int);
    nix::ioctl_readwrite!(vidioc_enumoutput, b'V', 48, v4l2_output);
}

#[derive(Debug, Error)]
pub enum SelectionError {
    #[error("selection {0} is out of range")]
    OutOfRange(usize),
    #[error("ioctl error: {0}")]
    IoctlError(Errno),
}

impl From<SelectionError> for Errno {
    fn from(err: SelectionError) -> Self {
        match err {
            SelectionError::OutOfRange(_) => Errno::EINVAL,
            SelectionError::IoctlError(e) => e,
        }
    }
}

/// Safe wrapper around the `VIDIOC_ENUMINPUT` ioctl.
pub fn enuminput<R: From<v4l2_input>>(
    fd: &impl AsRawFd,
    index: usize,
) -> Result<R, SelectionError> {
    let mut input = v4l2_input {
        index: index as u32,
        ..Default::default()
    };

    match unsafe { ioctl::vidioc_enuminput(fd.as_raw_fd(), &mut input) } {
        Ok(_) => Ok(R::from(input)),
        Err(Errno::EINVAL) => Err(SelectionError::OutOfRange(index)),
        Err(e) => Err(SelectionError::IoctlError(e)),
    }
}

/// Safe wrapper around the `VIDIOC_G_INPUT` ioctl.
pub fn g_input(fd: &impl AsRawFd) -> Result<usize, Errno> {
    let mut input: c_int = 0;

    unsafe { ioctl::vidioc_g_input(fd.as_raw_fd(), &mut input) }.map(|r| r as usize)
}

/// Safe wrapper around the `VIDIOC_S_INPUT` ioctl.
///
/// Returns the updated `index` upon success.
pub fn s_input(fd: &impl AsRawFd, index: usize) -> Result<usize, SelectionError> {
    let mut input: c_int = index as c_int;

    match unsafe { ioctl::vidioc_s_input(fd.as_raw_fd(), &mut input) } {
        Ok(_) => Ok(input as usize),
        Err(Errno::EINVAL) => Err(SelectionError::OutOfRange(index)),
        Err(e) => Err(SelectionError::IoctlError(e)),
    }
}

/// Safe wrapper around the `VIDIOC_ENUMOUTPUT` ioctl.
pub fn enumoutput<R: From<v4l2_output>>(
    fd: &impl AsRawFd,
    index: usize,
) -> Result<R, SelectionError> {
    let mut output = v4l2_output {
        index: index as u32,
        ..Default::default()
    };

    match unsafe { ioctl::vidioc_enumoutput(fd.as_raw_fd(), &mut output) } {
        Ok(_) => Ok(R::from(output)),
        Err(Errno::EINVAL) => Err(SelectionError::OutOfRange(index)),
        Err(e) => Err(SelectionError::IoctlError(e)),
    }
}

/// Safe wrapper around the `VIDIOC_G_OUTPUT` ioctl.
pub fn g_output(fd: &impl AsRawFd) -> Result<usize, Errno> {
    let mut output: c_int = 0;

    unsafe { ioctl::vidioc_g_output(fd.as_raw_fd(), &mut output) }.map(|r| r as usize)
}

/// Safe wrapper around the `VIDIOC_S_OUTPUT` ioctl.
pub fn s_output(fd: &impl AsRawFd, index: usize) -> Result<(), SelectionError> {
    let mut output: c_int = index as c_int;

    match unsafe { ioctl::vidioc_s_output(fd.as_raw_fd(), &mut output) } {
        Ok(_) => Ok(()),
        Err(Errno::EINVAL) => Err(SelectionError::OutOfRange(index)),
        Err(e) => Err(SelectionError::IoctlError(e)),
    }
}

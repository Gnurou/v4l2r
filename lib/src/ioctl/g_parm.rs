use std::os::unix::io::AsRawFd;

use nix::errno::Errno;
use thiserror::Error;

use crate::bindings::v4l2_streamparm;
use crate::QueueType;

#[doc(hidden)]
mod ioctl {
    use crate::bindings::v4l2_streamparm;

    nix::ioctl_readwrite!(vidioc_g_parm, b'V', 21, v4l2_streamparm);
    nix::ioctl_readwrite!(vidioc_s_parm, b'V', 22, v4l2_streamparm);
}

#[derive(Debug, Error)]
pub enum GParmError {
    #[error("ioctl error: {0}")]
    IoctlError(Errno),
}

impl From<GParmError> for Errno {
    fn from(err: GParmError) -> Self {
        match err {
            GParmError::IoctlError(e) => e,
        }
    }
}

/// Safe wrapper around the `VIDIOC_G_PARM` ioctl.
pub fn g_parm<O: From<v4l2_streamparm>>(
    fd: &impl AsRawFd,
    queue: QueueType,
) -> Result<O, GParmError> {
    let mut parm = v4l2_streamparm {
        type_: queue as u32,
        ..unsafe { std::mem::zeroed() }
    };

    match unsafe { ioctl::vidioc_g_parm(fd.as_raw_fd(), &mut parm) } {
        Ok(_) => Ok(O::from(parm)),
        Err(e) => Err(GParmError::IoctlError(e)),
    }
}

/// Safe wrapper around the `VIDIOC_S_PARM` ioctl.
pub fn s_parm<I: Into<v4l2_streamparm>, O: From<v4l2_streamparm>>(
    fd: &impl AsRawFd,
    parm: I,
) -> Result<O, GParmError> {
    let mut parm = parm.into();

    match unsafe { ioctl::vidioc_s_parm(fd.as_raw_fd(), &mut parm) } {
        Ok(_) => Ok(O::from(parm)),
        Err(e) => Err(GParmError::IoctlError(e)),
    }
}

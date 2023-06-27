use std::os::unix::io::AsRawFd;
use thiserror::Error;

use crate::{bindings, PixelFormat};

pub trait FrameIntervals {
    fn from(input: bindings::v4l2_frmivalenum) -> Self;
}

impl FrameIntervals for bindings::v4l2_frmivalenum {
    fn from(input: bindings::v4l2_frmivalenum) -> Self {
        input
    }
}

#[doc(hidden)]
mod ioctl {
    use crate::bindings::v4l2_frmivalenum;
    nix::ioctl_readwrite!(vidioc_enum_frameintervals, b'V', 75, v4l2_frmivalenum);
}

#[derive(Debug, Error)]
pub enum FrameIntervalsError {
    #[error("Unexpected ioctl error: {0}")]
    IoctlError(nix::Error),
}

pub fn enum_frame_intervals<T: FrameIntervals>(
    fd: &impl AsRawFd,
    index: u32,
    pixel_format: PixelFormat,
    width: u32,
    heigth: u32,
) -> Result<T, FrameIntervalsError> {
    let mut frame_interval = bindings::v4l2_frmivalenum {
        index,
        pixel_format: pixel_format.into(),
        width: width,
        height: heigth,
        ..unsafe { std::mem::zeroed() }
    };

    match unsafe { ioctl::vidioc_enum_frameintervals(fd.as_raw_fd(), &mut frame_interval) } {
        Ok(_) => Ok(T::from(frame_interval)),
        Err(e) => Err(FrameIntervalsError::IoctlError(e)),
    }
}

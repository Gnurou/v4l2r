use std::os::unix::io::AsRawFd;
use thiserror::Error;

use crate::{bindings, PixelFormat};

pub trait FrameSize {
    fn from(input: bindings::v4l2_frmsizeenum) -> Self;
}

#[doc(hidden)]
mod ioctl {
    use crate::bindings::v4l2_frmsizeenum;
    nix::ioctl_readwrite!(vidioc_enum_framesizes, b'V', 74, v4l2_frmsizeenum);
}


#[derive(Debug, Error)]
pub enum FrameSizeError {
    #[error("Unexpected ioctl error: {0}")]
    IoctlError(nix::Error),
}

pub fn enum_frame_sizes<T: FrameSize>(fd: &impl AsRawFd, index: u32, pixel_format: PixelFormat) -> Result<T, FrameSizeError>{
    let mut frame_size = bindings::v4l2_frmsizeenum {
        index,
        pixel_format: pixel_format.into(),
        ..unsafe {std::mem::zeroed()}
    };

        match unsafe {
            ioctl::vidioc_enum_framesizes(fd.as_raw_fd(), &mut frame_size)
        } {
            Ok(_) => Ok(T::from(frame_size)),
            Err(e) =>  Err(FrameSizeError::IoctlError(e))
        }

}
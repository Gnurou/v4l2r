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

/// A wrapper for the 'v4l2_frmivalenum' union member types
#[derive(Debug)]
pub enum FrmIvalTypes<'a> {
    Discrete(&'a bindings::v4l2_fract),
    StepWise(&'a bindings::v4l2_frmival_stepwise),
}

impl bindings::v4l2_frmivalenum {
    /// Safely access the intervals member of the struct based on the
    /// returned index.
    pub fn intervals(&self) -> Option<FrmIvalTypes> {
        match self.index {
            // SAFETY: the member of the union that gets used by the driver
            // is determined by the index
            bindings::v4l2_frmivaltypes_V4L2_FRMIVAL_TYPE_DISCRETE => {
                Some(FrmIvalTypes::Discrete(unsafe {
                    &self.__bindgen_anon_1.discrete
                }))
            }

            // SAFETY: the member of the union that gets used by the driver
            // is determined by the index
            bindings::v4l2_frmivaltypes_V4L2_FRMIVAL_TYPE_CONTINUOUS
            | bindings::v4l2_frmivaltypes_V4L2_FRMIVAL_TYPE_STEPWISE => {
                Some(FrmIvalTypes::StepWise(unsafe {
                    &self.__bindgen_anon_1.stepwise
                }))
            }

            _ => None,
        }
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
    height: u32,
) -> Result<T, FrameIntervalsError> {
    let mut frame_interval = bindings::v4l2_frmivalenum {
        index,
        pixel_format: pixel_format.into(),
        width,
        height,
        ..unsafe { std::mem::zeroed() }
    };

    match unsafe { ioctl::vidioc_enum_frameintervals(fd.as_raw_fd(), &mut frame_interval) } {
        Ok(_) => Ok(T::from(frame_interval)),
        Err(e) => Err(FrameIntervalsError::IoctlError(e)),
    }
}

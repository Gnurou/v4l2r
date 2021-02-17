use std::mem;
use std::os::unix::io::AsRawFd;

use nix::errno::Errno;
use thiserror::Error;

use crate::{bindings, Rect};

#[repr(u32)]
pub enum SelectionType {
    Capture = bindings::v4l2_buf_type_V4L2_BUF_TYPE_VIDEO_CAPTURE,
    Output = bindings::v4l2_buf_type_V4L2_BUF_TYPE_VIDEO_OUTPUT,
}

#[repr(u32)]
pub enum SelectionTarget {
    Crop = bindings::V4L2_SEL_TGT_CROP,
    CropDefault = bindings::V4L2_SEL_TGT_CROP_DEFAULT,
    CropBounds = bindings::V4L2_SEL_TGT_CROP_BOUNDS,
    NativeSize = bindings::V4L2_SEL_TGT_NATIVE_SIZE,
    Compose = bindings::V4L2_SEL_TGT_COMPOSE,
    ComposeDefault = bindings::V4L2_SEL_TGT_COMPOSE_DEFAULT,
    ComposeBounds = bindings::V4L2_SEL_TGT_COMPOSE_BOUNDS,
    ComposePadded = bindings::V4L2_SEL_TGT_COMPOSE_PADDED,
}

#[doc(hidden)]
mod ioctl {
    use crate::bindings::v4l2_selection;
    nix::ioctl_readwrite!(vidioc_g_selection, b'V', 94, v4l2_selection);
    nix::ioctl_readwrite!(vidioc_s_selection, b'V', 95, v4l2_selection);
}

#[derive(Debug, Error)]
pub enum GSelectionError {
    #[error("Invalid type or target requested")]
    Invalid,
    #[error("Unexpected ioctl error: {0}")]
    IoctlError(nix::Error),
}

pub fn g_selection<F: AsRawFd>(
    fd: &F,
    selection: SelectionType,
    target: SelectionTarget,
) -> Result<Rect, GSelectionError> {
    let mut sel = bindings::v4l2_selection {
        type_: selection as u32,
        target: target as u32,
        ..unsafe { mem::zeroed() }
    };

    match unsafe { ioctl::vidioc_g_selection(fd.as_raw_fd(), &mut sel) } {
        Ok(_) => Ok(Rect::from(sel.r)),
        Err(nix::Error::Sys(Errno::EINVAL)) => Err(GSelectionError::Invalid),
        Err(e) => Err(GSelectionError::IoctlError(e)),
    }
}

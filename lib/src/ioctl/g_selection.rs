use std::mem;
use std::os::unix::io::AsRawFd;

use enumn::N;
use nix::errno::Errno;
use thiserror::Error;

use crate::bindings;
use crate::bindings::v4l2_rect;
use crate::bindings::v4l2_selection;

#[derive(Debug, N)]
#[repr(u32)]
pub enum SelectionType {
    Capture = bindings::v4l2_buf_type_V4L2_BUF_TYPE_VIDEO_CAPTURE,
    Output = bindings::v4l2_buf_type_V4L2_BUF_TYPE_VIDEO_OUTPUT,
}

#[derive(Debug, N)]
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
    #[error("invalid type or target requested")]
    Invalid,
    #[error("ioctl error: {0}")]
    IoctlError(nix::Error),
}

pub fn g_selection<F: AsRawFd, R: From<v4l2_rect>>(
    fd: &F,
    selection: SelectionType,
    target: SelectionTarget,
) -> Result<R, GSelectionError> {
    let mut sel = v4l2_selection {
        type_: selection as u32,
        target: target as u32,
        ..unsafe { mem::zeroed() }
    };

    match unsafe { ioctl::vidioc_g_selection(fd.as_raw_fd(), &mut sel) } {
        Ok(_) => Ok(R::from(sel.r)),
        Err(Errno::EINVAL) => Err(GSelectionError::Invalid),
        Err(e) => Err(GSelectionError::IoctlError(e)),
    }
}

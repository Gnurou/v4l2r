//! Safe wrapper for the `VIDIOC_QUERYCTRL` and `VIDIOC_QUERY_EXT_CTRL` ioctls.
use std::os::unix::io::AsRawFd;

use bitflags::bitflags;
use nix::errno::Errno;
use thiserror::Error;

use crate::bindings;
use crate::bindings::v4l2_query_ext_ctrl;
use crate::bindings::v4l2_queryctrl;

/// Index of a control that has been validated, i.e. which ID is within the range of
/// `V4L2_CTRL_ID_MASK`.
#[derive(Debug, PartialEq, Eq)]
pub struct CtrlId(u32);

#[derive(Debug, Error, PartialEq, Eq)]
pub enum CtrlIdError {
    #[error("invalid control number: 0x{0:08x}")]
    InvalidControl(u32),
}

impl CtrlId {
    /// Create a new control index from its u32 representation, after validation.
    pub fn new(ctrl: u32) -> Result<Self, CtrlIdError> {
        if (ctrl & bindings::V4L2_CTRL_ID_MASK) != ctrl {
            Err(CtrlIdError::InvalidControl(ctrl))
        } else {
            Ok(CtrlId(ctrl))
        }
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug)]
    pub struct QueryCtrlFlags: u32 {
        const NEXT = bindings::V4L2_CTRL_FLAG_NEXT_CTRL;
        const COMPOUND = bindings::V4L2_CTRL_FLAG_NEXT_COMPOUND;
    }
}

/// Decompose a u32 between its control ID and query flags parts.
pub fn parse_ctrl_id_and_flags(ctrl: u32) -> (CtrlId, QueryCtrlFlags) {
    (
        CtrlId(ctrl & bindings::V4L2_CTRL_ID_MASK),
        QueryCtrlFlags::from_bits_truncate(ctrl),
    )
}

#[doc(hidden)]
mod ioctl {
    use crate::bindings::v4l2_queryctrl;
    nix::ioctl_readwrite!(vidioc_queryctrl, b'V', 36, v4l2_queryctrl);

    use crate::bindings::v4l2_query_ext_ctrl;
    nix::ioctl_readwrite!(vidioc_query_ext_ctrl, b'V', 103, v4l2_query_ext_ctrl);
}

#[derive(Debug, Error)]
pub enum QueryCtrlError {
    #[error("ioctl error: {0}")]
    IoctlError(Errno),
}

impl From<QueryCtrlError> for Errno {
    fn from(err: QueryCtrlError) -> Self {
        match err {
            QueryCtrlError::IoctlError(e) => e,
        }
    }
}

/// Safe wrapper around the `VIDIOC_QUERYCTRL` ioctl.
pub fn queryctrl<T: From<v4l2_queryctrl>>(
    fd: &impl AsRawFd,
    id: CtrlId,
    flags: QueryCtrlFlags,
) -> Result<T, QueryCtrlError> {
    let mut qctrl: v4l2_queryctrl = v4l2_queryctrl {
        id: id.0 | flags.bits(),
        ..Default::default()
    };

    match unsafe { ioctl::vidioc_queryctrl(fd.as_raw_fd(), &mut qctrl) } {
        Ok(_) => Ok(T::from(qctrl)),
        Err(e) => Err(QueryCtrlError::IoctlError(e)),
    }
}

/// Safe wrapper around the `VIDIOC_QUERYCTRL` ioctl.
pub fn query_ext_ctrl<T: From<v4l2_query_ext_ctrl>>(
    fd: &impl AsRawFd,
    id: CtrlId,
    flags: QueryCtrlFlags,
) -> Result<T, QueryCtrlError> {
    let mut qctrl: v4l2_query_ext_ctrl = v4l2_query_ext_ctrl {
        id: id.0 | flags.bits(),
        ..Default::default()
    };

    match unsafe { ioctl::vidioc_query_ext_ctrl(fd.as_raw_fd(), &mut qctrl) } {
        Ok(_) => Ok(T::from(qctrl)),
        Err(e) => Err(QueryCtrlError::IoctlError(e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ctrlid() {
        assert_eq!(
            CtrlId::new(bindings::V4L2_CID_AUDIO_VOLUME),
            Ok(CtrlId(bindings::V4L2_CID_AUDIO_VOLUME))
        );
        assert_eq!(
            CtrlId::new(bindings::V4L2_CTRL_FLAG_NEXT_CTRL),
            Err(CtrlIdError::InvalidControl(
                bindings::V4L2_CTRL_FLAG_NEXT_CTRL
            ))
        );
    }
}

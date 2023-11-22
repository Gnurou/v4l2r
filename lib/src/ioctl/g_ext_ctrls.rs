use nix::errno::Errno;
use std::convert::TryFrom;
use std::mem;
use std::os::unix::io::AsRawFd;
use std::os::unix::io::RawFd;
use thiserror::Error;

use crate::bindings;
use crate::bindings::v4l2_control;
use crate::bindings::v4l2_ctrl_fwht_params;
use crate::bindings::v4l2_ext_controls;
use crate::bindings::v4l2_querymenu;
use crate::controls::codec::FwhtFlags;
use crate::controls::AsV4l2ControlSlice;
use crate::Colorspace;
use crate::Quantization;
use crate::XferFunc;
use crate::YCbCrEncoding;

/// Kind of a codec control.
///
/// For controls with a pointer payload, this type also contains information like array sizes when
/// the size of the payload cannot be inferred from its ID alone.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ExtControlKind {
    // User Controls
    Brightness,
    Contrast,
    Saturation,
    // Codec controls
    FwhtParams,
    VP8Frame,
}

impl ExtControlKind {
    pub fn id(&self) -> u32 {
        match self {
            ExtControlKind::Brightness => bindings::V4L2_CID_BRIGHTNESS,
            ExtControlKind::Contrast => bindings::V4L2_CID_CONTRAST,
            ExtControlKind::Saturation => bindings::V4L2_CID_SATURATION,
            ExtControlKind::FwhtParams => bindings::V4L2_CID_STATELESS_FWHT_PARAMS,
            ExtControlKind::VP8Frame => bindings::V4L2_CID_STATELESS_VP8_FRAME,
        }
    }

    /// Returns the size of the payload for a control of this kind.
    ///
    /// This returns zero for controls that have no pointer payload.
    pub fn size(&self) -> usize {
        match self {
            ExtControlKind::Brightness | ExtControlKind::Contrast | ExtControlKind::Saturation => 0,
            ExtControlKind::FwhtParams => std::mem::size_of::<bindings::v4l2_ctrl_fwht_params>(),
            ExtControlKind::VP8Frame => std::mem::size_of::<bindings::v4l2_ctrl_vp8_frame>(),
        }
    }
}

/// A control that has been validated.
#[repr(transparent)]
pub struct ValidControl<T>(T);

#[derive(Debug, Error)]
pub enum FwhtParamsCtrlError {
    #[error("invalid flags: 0x{0:x}")]
    InvalidFlags(u32),
    #[error("invalid color space value: {0}")]
    InvalidColorspace(u32),
    #[error("invalid xfer func: {0}")]
    InvalidXferFunc(u32),
    #[error("invalid ycbcr encoding: {0}")]
    InvalidYCbCrEncoding(u32),
    #[error("invalid quantization: {0}")]
    InvalidQuantization(u32),
}

/// Make sure a FwhtParams control contains valid values.
impl TryFrom<v4l2_ctrl_fwht_params> for ValidControl<v4l2_ctrl_fwht_params> {
    type Error = FwhtParamsCtrlError;

    fn try_from(value: bindings::v4l2_ctrl_fwht_params) -> Result<Self, Self::Error> {
        // Validate all the input data.
        let _ = FwhtFlags::from_bits(value.flags)
            .ok_or(FwhtParamsCtrlError::InvalidFlags(value.flags))?;
        let _ = Colorspace::n(value.colorspace)
            .ok_or(FwhtParamsCtrlError::InvalidColorspace(value.colorspace))?;
        let _ = XferFunc::n(value.xfer_func)
            .ok_or(FwhtParamsCtrlError::InvalidXferFunc(value.xfer_func))?;
        let _ = YCbCrEncoding::n(value.ycbcr_enc)
            .ok_or(FwhtParamsCtrlError::InvalidYCbCrEncoding(value.ycbcr_enc))?;
        let _ = Quantization::n(value.quantization)
            .ok_or(FwhtParamsCtrlError::InvalidQuantization(value.quantization))?;

        Ok(ValidControl(value))
    }
}

impl ValidControl<v4l2_ctrl_fwht_params> {
    pub fn flags(&self) -> FwhtFlags {
        FwhtFlags::from_bits(self.0.flags).unwrap()
    }

    pub fn colorspace(&self) -> Colorspace {
        Colorspace::n(self.0.colorspace).unwrap()
    }

    pub fn xfer_func(&self) -> XferFunc {
        XferFunc::n(self.0.xfer_func).unwrap()
    }

    pub fn ycbcr_enc(&self) -> YCbCrEncoding {
        YCbCrEncoding::n(self.0.ycbcr_enc).unwrap()
    }

    pub fn quantization(&self) -> Quantization {
        Quantization::n(self.0.quantization).unwrap()
    }
}

#[doc(hidden)]
mod ioctl {
    use crate::bindings::v4l2_control;
    use crate::bindings::v4l2_ext_controls;
    use crate::bindings::v4l2_querymenu;
    nix::ioctl_readwrite!(vidioc_g_ctrl, b'V', 27, v4l2_control);
    nix::ioctl_readwrite!(vidioc_s_ctrl, b'V', 28, v4l2_control);
    nix::ioctl_readwrite!(vidioc_g_ext_ctrls, b'V', 71, v4l2_ext_controls);
    nix::ioctl_readwrite!(vidioc_s_ext_ctrls, b'V', 72, v4l2_ext_controls);
    nix::ioctl_readwrite!(vidioc_try_ext_ctrls, b'V', 73, v4l2_ext_controls);
    nix::ioctl_readwrite!(vidioc_querymenu, b'V', 37, v4l2_querymenu);
}

#[derive(Debug, Error)]
pub enum GCtrlError {
    #[error("Invalid control or value")]
    Invalid,
    #[error("Unexpected ioctl error: {0}")]
    IoctlError(nix::Error),
}

impl From<GCtrlError> for Errno {
    fn from(err: GCtrlError) -> Self {
        match err {
            GCtrlError::Invalid => Errno::EINVAL,
            GCtrlError::IoctlError(e) => e,
        }
    }
}

/// Safe wrapper around the `VIDIOC_G_CTRL` ioctl.
pub fn g_ctrl(fd: &impl AsRawFd, id: u32) -> Result<i32, GCtrlError> {
    let mut ctrl = v4l2_control {
        id,
        ..unsafe { std::mem::zeroed() }
    };

    match unsafe { ioctl::vidioc_g_ctrl(fd.as_raw_fd(), &mut ctrl) } {
        Ok(_) => Ok(ctrl.value),
        Err(Errno::EINVAL) => Err(GCtrlError::Invalid),
        Err(e) => Err(GCtrlError::IoctlError(e)),
    }
}

/// Safe wrapper around the `VIDIOC_S_CTRL` ioctl.
pub fn s_ctrl(fd: &impl AsRawFd, id: u32, value: i32) -> Result<i32, GCtrlError> {
    let mut ctrl = v4l2_control { id, value };

    match unsafe { ioctl::vidioc_s_ctrl(fd.as_raw_fd(), &mut ctrl) } {
        Ok(_) => Ok(ctrl.value),
        Err(Errno::EINVAL) => Err(GCtrlError::Invalid),
        Err(e) => Err(GCtrlError::IoctlError(e)),
    }
}

#[derive(Debug, Error)]
pub enum ExtControlErrorType {
    #[error("Unexpected ioctl error: {0}")]
    IoctlError(nix::Error),
}

impl From<ExtControlErrorType> for Errno {
    fn from(err: ExtControlErrorType) -> Self {
        match err {
            ExtControlErrorType::IoctlError(e) => e,
        }
    }
}

#[derive(Debug, Error)]
pub struct ExtControlError {
    pub error_idx: u32,
    pub error: ExtControlErrorType,
}

impl std::fmt::Display for ExtControlError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} at index {}", self.error, self.error_idx)
    }
}

impl From<ExtControlError> for Errno {
    fn from(err: ExtControlError) -> Self {
        Self::from(err.error)
    }
}

/// Encapsulates the `ctrl_class` and `which` enum of `v4l2_ext_controls`.
///
/// Note that `Default` is an invalid value for `S_EXT_CTRLS` and `TRY_EXT_CTRLS`.
pub enum CtrlWhich {
    Current,
    Default,
    Request(RawFd),
    Class(u32),
}

use bindings::v4l2_ext_controls__bindgen_ty_1 as v4l2_class_or_which;

impl CtrlWhich {
    fn binding_value(&self) -> v4l2_class_or_which {
        match self {
            CtrlWhich::Current => v4l2_class_or_which {
                which: bindings::V4L2_CTRL_WHICH_CUR_VAL,
            },
            CtrlWhich::Default => v4l2_class_or_which {
                which: bindings::V4L2_CTRL_WHICH_DEF_VAL,
            },
            CtrlWhich::Request(_) => v4l2_class_or_which {
                which: bindings::V4L2_CTRL_WHICH_REQUEST_VAL,
            },
            CtrlWhich::Class(class) => v4l2_class_or_which { ctrl_class: *class },
        }
    }
}

impl TryFrom<&v4l2_ext_controls> for CtrlWhich {
    type Error = ();

    fn try_from(ctrls: &v4l2_ext_controls) -> Result<Self, Self::Error> {
        let which_or_class = unsafe { ctrls.__bindgen_anon_1.which };
        match which_or_class {
            bindings::V4L2_CTRL_WHICH_CUR_VAL => Ok(CtrlWhich::Current),
            bindings::V4L2_CTRL_WHICH_DEF_VAL => Ok(CtrlWhich::Default),
            bindings::V4L2_CTRL_WHICH_REQUEST_VAL => Ok(CtrlWhich::Request(ctrls.request_fd)),
            bindings::V4L2_CTRL_CLASS_USER
            | bindings::V4L2_CTRL_CLASS_CODEC
            | bindings::V4L2_CTRL_CLASS_CAMERA
            | bindings::V4L2_CTRL_CLASS_FM_TX
            | bindings::V4L2_CTRL_CLASS_FLASH
            | bindings::V4L2_CTRL_CLASS_JPEG
            | bindings::V4L2_CTRL_CLASS_IMAGE_SOURCE
            | bindings::V4L2_CTRL_CLASS_IMAGE_PROC
            | bindings::V4L2_CTRL_CLASS_DV
            | bindings::V4L2_CTRL_CLASS_FM_RX
            | bindings::V4L2_CTRL_CLASS_RF_TUNER
            | bindings::V4L2_CTRL_CLASS_DETECT
            | bindings::V4L2_CTRL_CLASS_CODEC_STATELESS
            | bindings::V4L2_CTRL_CLASS_COLORIMETRY => Ok(CtrlWhich::Class(which_or_class)),
            _ => Err(()),
        }
    }
}

/// Safe wrapper around the `VIDIOC_G_EXT_CTRLS` to get the value of extended controls.
///
/// If successful, values for the controls will be written in the `controls` parameter.
pub fn g_ext_ctrls<I: AsV4l2ControlSlice>(
    fd: &impl AsRawFd,
    which: CtrlWhich,
    mut controls: I,
) -> Result<(), ExtControlError> {
    let controls_slice = controls.as_v4l2_control_slice();
    let mut v4l2_controls = v4l2_ext_controls {
        __bindgen_anon_1: which.binding_value(),
        count: controls_slice.len() as u32,
        request_fd: if let CtrlWhich::Request(fd) = which {
            fd
        } else {
            0
        },
        controls: controls_slice.as_mut_ptr(),
        // SAFETY: ok to zero-fill this struct, the pointer it contains will be assigned to in this function
        ..unsafe { mem::zeroed() }
    };

    // SAFETY: the 'controls' argument is properly set up above
    match unsafe { ioctl::vidioc_g_ext_ctrls(fd.as_raw_fd(), &mut v4l2_controls) } {
        Ok(_) => Ok(()),
        Err(e) => Err(ExtControlError {
            error_idx: v4l2_controls.error_idx,
            error: ExtControlErrorType::IoctlError(e),
        }),
    }
}

/// Safe wrapper around the `VIDIOC_S_EXT_CTRLS` to set the value of extended controls.
pub fn s_ext_ctrls<I: AsV4l2ControlSlice>(
    fd: &impl AsRawFd,
    which: CtrlWhich,
    mut controls: I,
) -> Result<(), ExtControlError> {
    let controls_slice = controls.as_v4l2_control_slice();
    let mut v4l2_controls = v4l2_ext_controls {
        __bindgen_anon_1: which.binding_value(),
        count: controls_slice.len() as u32,
        request_fd: if let CtrlWhich::Request(fd) = which {
            fd
        } else {
            0
        },
        controls: controls_slice.as_mut_ptr(),
        // SAFETY: ok to zero-fill this struct, the pointer it contains will be assigned to in this function
        ..unsafe { mem::zeroed() }
    };

    // SAFETY: the 'controls' argument is properly set up above
    match unsafe { ioctl::vidioc_s_ext_ctrls(fd.as_raw_fd(), &mut v4l2_controls) } {
        Ok(_) => Ok(()),
        Err(e) => Err(ExtControlError {
            error_idx: v4l2_controls.error_idx,
            error: ExtControlErrorType::IoctlError(e),
        }),
    }
}

/// Safe wrapper around the `VIDIOC_TRY_EXT_CTRLS` to test the value of extended controls.
pub fn try_ext_ctrls<I: AsV4l2ControlSlice>(
    fd: &impl AsRawFd,
    which: CtrlWhich,
    mut controls: I,
) -> Result<(), ExtControlError> {
    let controls_slice = controls.as_v4l2_control_slice();
    let mut v4l2_controls = v4l2_ext_controls {
        __bindgen_anon_1: which.binding_value(),
        count: controls_slice.len() as u32,
        request_fd: if let CtrlWhich::Request(fd) = which {
            fd
        } else {
            0
        },
        controls: controls_slice.as_mut_ptr(),
        // SAFETY: ok to zero-fill this struct, the pointer it contains will be assigned to in this function
        ..unsafe { mem::zeroed() }
    };

    // SAFETY: the 'controls' argument is properly set up above
    match unsafe { ioctl::vidioc_try_ext_ctrls(fd.as_raw_fd(), &mut v4l2_controls) } {
        Ok(_) => Ok(()),
        Err(e) => Err(ExtControlError {
            error_idx: v4l2_controls.error_idx,
            error: ExtControlErrorType::IoctlError(e),
        }),
    }
}

#[derive(Debug, Error)]
pub enum QueryMenuError {
    #[error("invalid id or index value")]
    InvalidIdOrIndex,
    #[error("ioctl error: {0}")]
    IoctlError(nix::Error),
}

impl From<QueryMenuError> for Errno {
    fn from(err: QueryMenuError) -> Self {
        match err {
            QueryMenuError::InvalidIdOrIndex => Errno::EINVAL,
            QueryMenuError::IoctlError(e) => e,
        }
    }
}

/// Safe wrapper around the `VIDIOC_QUERYMENU`
pub fn querymenu<O: From<v4l2_querymenu>>(
    fd: &impl AsRawFd,
    id: u32,
    index: u32,
) -> Result<O, QueryMenuError> {
    let mut querymenu = v4l2_querymenu {
        id,
        index,
        ..unsafe { std::mem::zeroed() }
    };

    match unsafe { ioctl::vidioc_querymenu(fd.as_raw_fd(), &mut querymenu) } {
        Ok(_) => Ok(querymenu.into()),
        Err(Errno::EINVAL) => Err(QueryMenuError::InvalidIdOrIndex),
        Err(e) => Err(QueryMenuError::IoctlError(e)),
    }
}

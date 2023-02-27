use bitflags::bitflags;
use enumn::N;
use std::convert::TryFrom;
use std::mem;
use std::os::unix::io::AsRawFd;
use thiserror::Error;

use crate::bindings;

#[repr(u32)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ExtControlKind {
    FwhtParams = bindings::V4L2_CID_STATELESS_FWHT_PARAMS,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Error)]
pub enum DataConversionError {
    #[error("Cannot convert colorspace")]
    ColorspaceErr,
    #[error("Cannot convert xfer function")]
    XferFuncErr,
    #[error("Cannot convert YCbCr encoding")]
    YCbCrEncodingErr,
    #[error("Cannot convert quantization")]
    QuantizationErr,
}

#[repr(u32)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, N)]
pub enum Colorspace {
    Default = bindings::v4l2_colorspace_V4L2_COLORSPACE_DEFAULT,
    Smpte170M = bindings::v4l2_colorspace_V4L2_COLORSPACE_SMPTE170M,
    Smpte240M = bindings::v4l2_colorspace_V4L2_COLORSPACE_SMPTE240M,
    Rec709 = bindings::v4l2_colorspace_V4L2_COLORSPACE_REC709,
    Bt878 = bindings::v4l2_colorspace_V4L2_COLORSPACE_BT878,
    SystemM470 = bindings::v4l2_colorspace_V4L2_COLORSPACE_470_SYSTEM_M,
    SystemBG470 = bindings::v4l2_colorspace_V4L2_COLORSPACE_470_SYSTEM_BG,
    Jpeg = bindings::v4l2_colorspace_V4L2_COLORSPACE_JPEG,
    Srgb = bindings::v4l2_colorspace_V4L2_COLORSPACE_SRGB,
    OpRgb = bindings::v4l2_colorspace_V4L2_COLORSPACE_OPRGB,
    Bt2020 = bindings::v4l2_colorspace_V4L2_COLORSPACE_BT2020,
    Raw = bindings::v4l2_colorspace_V4L2_COLORSPACE_RAW,
    DciP3 = bindings::v4l2_colorspace_V4L2_COLORSPACE_DCI_P3,
}

#[repr(u32)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, N)]
pub enum XferFunc {
    Default = bindings::v4l2_xfer_func_V4L2_XFER_FUNC_DEFAULT,
    F709 = bindings::v4l2_xfer_func_V4L2_XFER_FUNC_709,
    Srgb = bindings::v4l2_xfer_func_V4L2_XFER_FUNC_SRGB,
    OpRgb = bindings::v4l2_xfer_func_V4L2_XFER_FUNC_OPRGB,
    Smpte240M = bindings::v4l2_xfer_func_V4L2_XFER_FUNC_SMPTE240M,
    None = bindings::v4l2_xfer_func_V4L2_XFER_FUNC_NONE,
    DciP3 = bindings::v4l2_xfer_func_V4L2_XFER_FUNC_DCI_P3,
    Smpte2084 = bindings::v4l2_xfer_func_V4L2_XFER_FUNC_SMPTE2084,
}

#[repr(u32)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, N)]
pub enum YCbCrEncoding {
    Default = bindings::v4l2_ycbcr_encoding_V4L2_YCBCR_ENC_DEFAULT,
    E601 = bindings::v4l2_ycbcr_encoding_V4L2_YCBCR_ENC_601,
    E709 = bindings::v4l2_ycbcr_encoding_V4L2_YCBCR_ENC_709,
    Xv601 = bindings::v4l2_ycbcr_encoding_V4L2_YCBCR_ENC_XV601,
    Xv709 = bindings::v4l2_ycbcr_encoding_V4L2_YCBCR_ENC_XV709,
    Sycc = bindings::v4l2_ycbcr_encoding_V4L2_YCBCR_ENC_SYCC,
    Bt2020 = bindings::v4l2_ycbcr_encoding_V4L2_YCBCR_ENC_BT2020,
    Bt2020ConstLum = bindings::v4l2_ycbcr_encoding_V4L2_YCBCR_ENC_BT2020_CONST_LUM,
    Smpte240M = bindings::v4l2_ycbcr_encoding_V4L2_YCBCR_ENC_SMPTE240M,
}

#[repr(u32)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, N)]
pub enum Quantization {
    Default = bindings::v4l2_quantization_V4L2_QUANTIZATION_DEFAULT,
    FullRange = bindings::v4l2_quantization_V4L2_QUANTIZATION_FULL_RANGE,
    LimRange = bindings::v4l2_quantization_V4L2_QUANTIZATION_LIM_RANGE,
}

bitflags! {
    /// FWHT Flags.
    pub struct FwhtFlags: u32 {
        const INTERLACED = bindings::V4L2_FWHT_FL_IS_INTERLACED as u32;
        const BOTTOM_FIRST = bindings::V4L2_FWHT_FL_IS_BOTTOM_FIRST as u32;
        const ALTERNATE = bindings::V4L2_FWHT_FL_IS_ALTERNATE as u32;
        const BOTTOM_FIELD = bindings::V4L2_FWHT_FL_IS_BOTTOM_FIELD as u32;
        const UNCOMPRESSED = bindings::V4L2_FWHT_FL_LUMA_IS_UNCOMPRESSED as u32;
        const CB_COMPRESSED = bindings::V4L2_FWHT_FL_CB_IS_UNCOMPRESSED as u32;
        const CR_COMPRESSED = bindings::V4L2_FWHT_FL_CR_IS_UNCOMPRESSED as u32;
        const CHROMA_FULL_HEIGHT = bindings::V4L2_FWHT_FL_CHROMA_FULL_HEIGHT as u32;
        const CHROMA_FULL_WIDTH = bindings::V4L2_FWHT_FL_CHROMA_FULL_WIDTH as u32;
        const ALPHA_UNCOMPRESSED = bindings::V4L2_FWHT_FL_ALPHA_IS_UNCOMPRESSED as u32;
        const I_FRAME = bindings::V4L2_FWHT_FL_I_FRAME as u32;
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct FwhtParams {
    pub backward_ref_ts: u64,
    pub version: u32,
    pub width: u32,
    pub height: u32,
    pub flags: u32,
    pub components: u32,
    pub encoding: u32,
    pub colorspace: Colorspace,
    pub xfer_func: XferFunc,
    pub ycbcr_enc: YCbCrEncoding,
    pub quantization: Quantization,
}

// Private enum serving as storage for control-specific bindgen struct
#[repr(C)]
#[allow(clippy::large_enum_variant)]
enum CompoundControl {
    FwhtParams(bindings::v4l2_ctrl_fwht_params),
}

impl CompoundControl {
    fn size(&self) -> usize {
        match self {
            CompoundControl::FwhtParams(inner) => std::mem::size_of_val(inner),
        }
    }

    fn as_mut_ptr(&mut self) -> *mut std::ffi::c_void {
        match self {
            CompoundControl::FwhtParams(inner) => inner as *mut _ as *mut std::ffi::c_void,
        }
    }
}

impl TryFrom<ExtControlKind> for CompoundControl {
    type Error = ExtControlError;

    fn try_from(value: ExtControlKind) -> Result<Self, Self::Error> {
        match value {
            ExtControlKind::FwhtParams => Ok(CompoundControl::FwhtParams(
                bindings::v4l2_ctrl_fwht_params {
                    // SAFETY: ok to zero-fill a struct which does not contain pointers/references
                    ..unsafe { mem::zeroed() }
                },
            )),
        }
    }
}

impl TryFrom<&ExtControl> for CompoundControl {
    type Error = ExtControlError;

    fn try_from(value: &ExtControl) -> Result<Self, Self::Error> {
        match *value {
            ExtControl::FwhtParams(fwht_params) => Ok(CompoundControl::FwhtParams(
                bindings::v4l2_ctrl_fwht_params {
                    backward_ref_ts: fwht_params.backward_ref_ts,
                    version: fwht_params.version,
                    width: fwht_params.width,
                    height: fwht_params.height,
                    flags: fwht_params.flags
                        | ((fwht_params.components
                            << bindings::V4L2_FWHT_FL_COMPONENTS_NUM_OFFSET)
                            & bindings::V4L2_FWHT_FL_COMPONENTS_NUM_MSK)
                        | ((fwht_params.encoding << bindings::V4L2_FWHT_FL_PIXENC_OFFSET)
                            & bindings::V4L2_FWHT_FL_PIXENC_MSK),
                    colorspace: fwht_params.colorspace as u32,
                    xfer_func: fwht_params.xfer_func as u32,
                    ycbcr_enc: fwht_params.ycbcr_enc as u32,
                    quantization: fwht_params.quantization as u32,
                },
            )),
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ExtControl {
    FwhtParams(FwhtParams),
}

impl ExtControl {
    pub fn kind(&self) -> ExtControlKind {
        match &self {
            ExtControl::FwhtParams(_) => ExtControlKind::FwhtParams,
        }
    }
}

impl TryFrom<CompoundControl> for ExtControl {
    type Error = DataConversionError;

    fn try_from(ctrl: CompoundControl) -> Result<Self, Self::Error> {
        match ctrl {
            CompoundControl::FwhtParams(inner) => Ok(ExtControl::FwhtParams(FwhtParams {
                backward_ref_ts: inner.backward_ref_ts,
                version: inner.version,
                width: inner.width,
                height: inner.height,
                flags: inner.flags,
                components: (inner.flags & bindings::V4L2_FWHT_FL_COMPONENTS_NUM_MSK)
                    >> bindings::V4L2_FWHT_FL_COMPONENTS_NUM_OFFSET,
                encoding: (inner.flags & bindings::V4L2_FWHT_FL_PIXENC_MSK)
                    >> bindings::V4L2_FWHT_FL_PIXENC_OFFSET,
                colorspace: Colorspace::n(inner.colorspace).ok_or(Self::Error::ColorspaceErr)?,
                xfer_func: XferFunc::n(inner.xfer_func).ok_or(Self::Error::XferFuncErr)?,
                ycbcr_enc: YCbCrEncoding::n(inner.ycbcr_enc)
                    .ok_or(Self::Error::YCbCrEncodingErr)?,
                quantization: Quantization::n(inner.quantization)
                    .ok_or(Self::Error::QuantizationErr)?,
            })),
        }
    }
}

#[doc(hidden)]
mod ioctl {
    use crate::bindings::v4l2_ext_controls;
    nix::ioctl_readwrite!(vidioc_g_ext_ctrls, b'V', 71, v4l2_ext_controls);
    nix::ioctl_readwrite!(vidioc_s_ext_ctrls, b'V', 72, v4l2_ext_controls);
}

#[derive(Debug, Error)]
pub enum ExtControlError {
    #[error("Unexpected ioctl error: {0}")]
    IoctlError(nix::Error),
    #[error("This control is not a compound control")]
    NotACompoundControlError,
    #[error("Failed data conversion: {0}")]
    DataConversionError(DataConversionError),
}

impl From<DataConversionError> for ExtControlError {
    fn from(err: DataConversionError) -> Self {
        ExtControlError::DataConversionError(err)
    }
}

/// Get a single extended control
///
/// While vidioc_g_ext_ctrls() accepts an array of controls,
/// the kernel internally seems not to commit all the values atomically,
/// and proceeds only until the first failure.
///
/// Given the above we provide an interface for querying a single control,
/// as this greatly simplifies the code.
pub fn g_ext_ctrl<F: AsRawFd>(
    fd: &F,
    ctrl_kind: ExtControlKind,
) -> Result<ExtControl, ExtControlError> {
    let mut control = bindings::v4l2_ext_control {
        // SAFETY: ok to zero-fill this struct, the pointer it contains will be assigned to in this function
        ..unsafe { mem::zeroed() }
    };
    let mut controls = bindings::v4l2_ext_controls {
        // SAFETY: ok to zero-fill this struct, the pointer it contains will be assigned to in this function
        ..unsafe { mem::zeroed() }
    };

    let mut compound_ctrl = CompoundControl::try_from(ctrl_kind)?;

    control.id = ctrl_kind as u32;
    control.size = compound_ctrl.size() as u32;
    // the pointer is assigned a proper value
    control.__bindgen_anon_1.ptr = compound_ctrl.as_mut_ptr();

    controls.__bindgen_anon_1.which = bindings::V4L2_CTRL_WHICH_CUR_VAL;
    controls.count = 1;
    // the pointer is assigned a proper value
    controls.controls = &mut control;

    // SAFETY: the 'controls' argument is properly set up above
    match unsafe { ioctl::vidioc_g_ext_ctrls(fd.as_raw_fd(), &mut controls) } {
        Ok(_) => Ok(ExtControl::try_from(compound_ctrl)?),
        Err(e) => Err(ExtControlError::IoctlError(e)),
    }
}

/// Set a single extended control
///
/// While vidioc_s_ext_ctrls() accepts an array of controls,
/// the kernel internally seems not to commit all the values atomically,
/// and proceeds only until the first failure.
///
/// Given the above we provide an interface for setting a single control,
/// as this greatly simplifies the code.
pub fn s_ext_ctrl<F: AsRawFd>(
    fd: &F,
    ctrl: &ExtControl,
) -> Result<ExtControl, ExtControlError> {
    let mut control = bindings::v4l2_ext_control {
        // SAFETY: ok to zero-fill this struct, the pointer it contains will be assigned to in this function
        ..unsafe { mem::zeroed() }
    };
    let mut controls = bindings::v4l2_ext_controls {
        // SAFETY: ok to fill this struct, the pointer it contains will be assigned to in this function
        ..unsafe { mem::zeroed() }
    };

    let mut compound_ctrl = CompoundControl::try_from(ctrl)?;

    control.id = ctrl.kind() as u32;
    control.size = compound_ctrl.size() as u32;
    // the pointer is assigned a proper value
    control.__bindgen_anon_1.ptr = compound_ctrl.as_mut_ptr();

    controls.__bindgen_anon_1.which = bindings::V4L2_CTRL_WHICH_CUR_VAL;
    controls.count = 1;
    // the pointer is assigned a proper value
    controls.controls = &mut control;

    // SAFETY: the 'controls' argument is properly set up above
    match unsafe { ioctl::vidioc_s_ext_ctrls(fd.as_raw_fd(), &mut controls) } {
        Ok(_) => Ok(ExtControl::try_from(compound_ctrl)?),
        Err(e) => Err(ExtControlError::IoctlError(e)),
    }
}

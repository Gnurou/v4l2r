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
    VP8Frame = bindings::V4L2_CID_STATELESS_VP8_FRAME,
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

bitflags! {
    /// VP8 Segment Flags.
    pub struct VP8SegmentFlags: u32 {
        const ENABLED = bindings::V4L2_VP8_SEGMENT_FLAG_ENABLED;
        const UPDATE_MAP = bindings::V4L2_VP8_SEGMENT_FLAG_UPDATE_MAP;
        const UPDATE_FEATURE_DATA = bindings::V4L2_VP8_SEGMENT_FLAG_UPDATE_FEATURE_DATA;
        const DELTA_VALUE_MODE = bindings::V4L2_VP8_SEGMENT_FLAG_DELTA_VALUE_MODE;
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct VP8Segment {
    pub quant_update: [i8; 4],
    pub lf_update: [i8; 4],
    pub segment_probs: [u8; 3],
    pub flags: u32,
}

impl From<bindings::v4l2_vp8_segment> for VP8Segment {
    fn from(value: bindings::v4l2_vp8_segment) -> Self {
        VP8Segment {
            quant_update: value.quant_update,
            lf_update: value.lf_update,
            segment_probs: value.segment_probs,
            flags: value.flags,
        }
    }
}

impl From<VP8Segment> for bindings::v4l2_vp8_segment {
    fn from(value: VP8Segment) -> Self {
        bindings::v4l2_vp8_segment {
            quant_update: value.quant_update,
            lf_update: value.lf_update,
            segment_probs: value.segment_probs,
            padding: 0,
            flags: value.flags,
        }
    }
}

bitflags! {
    /// VP8 Loop Filter Flags.
    pub struct VP8LoopFilterFlags: u32 {
        const ADJ_ENABLE = bindings::V4L2_VP8_LF_ADJ_ENABLE;
        const DELTA_UPDATE = bindings::V4L2_VP8_LF_DELTA_UPDATE;
        const FILTER_TYPE_SIMPLE = bindings::V4L2_VP8_LF_FILTER_TYPE_SIMPLE;
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct VP8LoopFilter {
    pub ref_frm_delta: [i8; 4],
    pub mb_mode_delta: [i8; 4],
    pub sharpness_level: u8,
    pub level: u8,
    pub flags: u32,
}

impl From<bindings::v4l2_vp8_loop_filter> for VP8LoopFilter {
    fn from(value: bindings::v4l2_vp8_loop_filter) -> Self {
        VP8LoopFilter {
            ref_frm_delta: value.ref_frm_delta,
            mb_mode_delta: value.mb_mode_delta,
            sharpness_level: value.sharpness_level,
            level: value.level,
            flags: value.flags,
        }
    }
}

impl From<VP8LoopFilter> for bindings::v4l2_vp8_loop_filter {
    fn from(value: VP8LoopFilter) -> Self {
        bindings::v4l2_vp8_loop_filter {
            ref_frm_delta: value.ref_frm_delta,
            mb_mode_delta: value.mb_mode_delta,
            sharpness_level: value.sharpness_level,
            level: value.level,
            padding: 0,
            flags: value.flags,
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct VP8Quantization {
    pub y_ac_qi: u8,
    pub y_dc_delta: i8,
    pub y2_dc_delta: i8,
    pub y2_ac_delta: i8,
    pub uv_dc_delta: i8,
    pub uv_ac_delta: i8,
}

impl From<bindings::v4l2_vp8_quantization> for VP8Quantization {
    fn from(value: bindings::v4l2_vp8_quantization) -> Self {
        VP8Quantization {
            y_ac_qi: value.y_ac_qi,
            y_dc_delta: value.y_dc_delta,
            y2_dc_delta: value.y2_dc_delta,
            y2_ac_delta: value.y2_ac_delta,
            uv_dc_delta: value.uv_dc_delta,
            uv_ac_delta: value.uv_ac_delta,
        }
    }
}

impl From<VP8Quantization> for bindings::v4l2_vp8_quantization {
    fn from(value: VP8Quantization) -> Self {
        bindings::v4l2_vp8_quantization {
            y_ac_qi: value.y_ac_qi,
            y_dc_delta: value.y_dc_delta,
            y2_dc_delta: value.y2_dc_delta,
            y2_ac_delta: value.y2_ac_delta,
            uv_dc_delta: value.uv_dc_delta,
            uv_ac_delta: value.uv_ac_delta,
            padding: 0,
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct VP8Entropy {
    pub coeff_probs: [[[[u8; 11]; 3]; 8]; 4],
    pub y_mode_probs: [u8; 4],
    pub uv_mode_probs: [u8; 3],
    pub mv_probs: [[u8; 19]; 2],
}

impl From<bindings::v4l2_vp8_entropy> for VP8Entropy {
    fn from(value: bindings::v4l2_vp8_entropy) -> Self {
        VP8Entropy {
            coeff_probs: value.coeff_probs,
            y_mode_probs: value.y_mode_probs,
            uv_mode_probs: value.uv_mode_probs,
            mv_probs: value.mv_probs,
        }
    }
}

impl From<VP8Entropy> for bindings::v4l2_vp8_entropy {
    fn from(value: VP8Entropy) -> Self {
        bindings::v4l2_vp8_entropy {
            coeff_probs: value.coeff_probs,
            y_mode_probs: value.y_mode_probs,
            uv_mode_probs: value.uv_mode_probs,
            mv_probs: value.mv_probs,
            padding: [0; 3],
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct VP8EntropyCoderState {
    pub range: u8,
    pub value: u8,
    pub bit_count: u8,
}

impl From<bindings::v4l2_vp8_entropy_coder_state> for VP8EntropyCoderState {
    fn from(value: bindings::v4l2_vp8_entropy_coder_state) -> Self {
        VP8EntropyCoderState {
            range: value.range,
            value: value.value,
            bit_count: value.bit_count,
        }
    }
}

impl From<VP8EntropyCoderState> for bindings::v4l2_vp8_entropy_coder_state {
    fn from(value: VP8EntropyCoderState) -> Self {
        bindings::v4l2_vp8_entropy_coder_state {
            range: value.range,
            value: value.value,
            bit_count: value.bit_count,
            padding: 0,
        }
    }
}

bitflags! {
    /// VP8 Frame Flags.
    pub struct VP8FrameFlags: u32 {
        const KEY_FRAME = bindings::V4L2_VP8_FRAME_FLAG_KEY_FRAME;
        const EXPERIMENTAL = bindings::V4L2_VP8_FRAME_FLAG_EXPERIMENTAL;
        const SHOW_FRAME = bindings::V4L2_VP8_FRAME_FLAG_SHOW_FRAME;
        const NO_SKIP_COEFF = bindings::V4L2_VP8_FRAME_FLAG_MB_NO_SKIP_COEFF;
        const SIGN_BIAS_GOLDEN = bindings::V4L2_VP8_FRAME_FLAG_SIGN_BIAS_GOLDEN;
        const SIGN_BIAS_ALT  = bindings::V4L2_VP8_FRAME_FLAG_SIGN_BIAS_ALT;
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct VP8Frame {
    pub segment: VP8Segment,
    pub lf: VP8LoopFilter,
    pub quant: VP8Quantization,
    pub entropy: VP8Entropy,
    pub coder_state: VP8EntropyCoderState,
    pub width: u16,
    pub height: u16,
    pub horizontal_scale: u8,
    pub vertical_scale: u8,
    pub version: u8,
    pub prob_skip_false: u8,
    pub prob_intra: u8,
    pub prob_last: u8,
    pub prob_gf: u8,
    pub num_dct_parts: u8,
    pub first_part_size: u32,
    pub first_part_header_bits: u32,
    pub dct_part_sizes: [u32; 8],
    pub last_frame_ts: u64,
    pub golden_frame_ts: u64,
    pub alt_frame_ts: u64,
    pub flags: u64,
}

// Private enum serving as storage for control-specific bindgen struct
#[repr(C)]
#[allow(clippy::large_enum_variant)]
enum CompoundControl {
    FwhtParams(bindings::v4l2_ctrl_fwht_params),
    VP8Frame(bindings::v4l2_ctrl_vp8_frame),
}

impl CompoundControl {
    fn size(&self) -> usize {
        match self {
            CompoundControl::FwhtParams(inner) => std::mem::size_of_val(inner),
            CompoundControl::VP8Frame(inner) => std::mem::size_of_val(inner),
        }
    }

    fn as_mut_ptr(&mut self) -> *mut std::ffi::c_void {
        match self {
            CompoundControl::FwhtParams(inner) => inner as *mut _ as *mut std::ffi::c_void,
            CompoundControl::VP8Frame(inner) => inner as *mut _ as *mut std::ffi::c_void,
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
            ExtControlKind::VP8Frame => {
                Ok(CompoundControl::VP8Frame(bindings::v4l2_ctrl_vp8_frame {
                    // SAFETY: ok to zero-fill a struct which does not contain pointers/references
                    ..unsafe { mem::zeroed() }
                }))
            }
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
            ExtControl::VP8Frame(vp8_frame) => {
                Ok(CompoundControl::VP8Frame(bindings::v4l2_ctrl_vp8_frame {
                    segment: bindings::v4l2_vp8_segment::from(vp8_frame.segment),
                    lf: bindings::v4l2_vp8_loop_filter::from(vp8_frame.lf),
                    quant: bindings::v4l2_vp8_quantization::from(vp8_frame.quant),
                    entropy: bindings::v4l2_vp8_entropy::from(vp8_frame.entropy),
                    coder_state: bindings::v4l2_vp8_entropy_coder_state::from(
                        vp8_frame.coder_state,
                    ),
                    width: vp8_frame.width,
                    height: vp8_frame.height,
                    horizontal_scale: vp8_frame.horizontal_scale,
                    vertical_scale: vp8_frame.vertical_scale,
                    version: vp8_frame.version,
                    prob_skip_false: vp8_frame.prob_skip_false,
                    prob_intra: vp8_frame.prob_intra,
                    prob_last: vp8_frame.prob_last,
                    prob_gf: vp8_frame.prob_gf,
                    num_dct_parts: vp8_frame.num_dct_parts,
                    first_part_size: vp8_frame.first_part_size,
                    first_part_header_bits: vp8_frame.first_part_header_bits,
                    dct_part_sizes: vp8_frame.dct_part_sizes,
                    last_frame_ts: vp8_frame.last_frame_ts,
                    golden_frame_ts: vp8_frame.golden_frame_ts,
                    alt_frame_ts: vp8_frame.alt_frame_ts,
                    flags: vp8_frame.flags,
                }))
            }
        }
    }
}

#[allow(clippy::large_enum_variant)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ExtControl {
    FwhtParams(FwhtParams),
    VP8Frame(VP8Frame),
}

impl ExtControl {
    pub fn kind(&self) -> ExtControlKind {
        match &self {
            ExtControl::FwhtParams(_) => ExtControlKind::FwhtParams,
            ExtControl::VP8Frame(_) => ExtControlKind::VP8Frame,
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
            CompoundControl::VP8Frame(inner) => Ok(ExtControl::VP8Frame(VP8Frame {
                segment: VP8Segment::from(inner.segment),
                lf: VP8LoopFilter::from(inner.lf),
                quant: VP8Quantization::from(inner.quant),
                entropy: VP8Entropy::from(inner.entropy),
                coder_state: VP8EntropyCoderState::from(inner.coder_state),
                width: inner.width,
                height: inner.height,
                horizontal_scale: inner.horizontal_scale,
                vertical_scale: inner.vertical_scale,
                version: inner.version,
                prob_skip_false: inner.prob_skip_false,
                prob_intra: inner.prob_intra,
                prob_last: inner.prob_last,
                prob_gf: inner.prob_gf,
                num_dct_parts: inner.num_dct_parts,
                first_part_size: inner.first_part_size,
                first_part_header_bits: inner.first_part_header_bits,
                dct_part_sizes: inner.dct_part_sizes,
                last_frame_ts: inner.last_frame_ts,
                golden_frame_ts: inner.golden_frame_ts,
                alt_frame_ts: inner.alt_frame_ts,
                flags: inner.flags,
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
pub fn g_ext_ctrl<R: AsRawFd>(
    fd: &impl AsRawFd,
    request_fd: Option<&R>,
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
    if let Some(request_fd) = request_fd {
        controls.request_fd = request_fd.as_raw_fd();
        controls.__bindgen_anon_1.which = bindings::V4L2_CTRL_WHICH_REQUEST_VAL;
    }

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
pub fn s_ext_ctrl<R: AsRawFd>(
    fd: &impl AsRawFd,
    request_fd: Option<&R>,
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
    if let Some(request_fd) = request_fd {
        controls.request_fd = request_fd.as_raw_fd();
        controls.__bindgen_anon_1.which = bindings::V4L2_CTRL_WHICH_REQUEST_VAL;
    }

    // SAFETY: the 'controls' argument is properly set up above
    match unsafe { ioctl::vidioc_s_ext_ctrls(fd.as_raw_fd(), &mut controls) } {
        Ok(_) => Ok(ExtControl::try_from(compound_ctrl)?),
        Err(e) => Err(ExtControlError::IoctlError(e)),
    }
}

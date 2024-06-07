//! Definition of CODEC class controls.

use bitflags::bitflags;
use enumn::N;

use crate::bindings;
use crate::bindings::v4l2_ctrl_fwht_params;
use crate::bindings::v4l2_ctrl_h264_decode_params;
use crate::bindings::v4l2_ctrl_h264_pps;
use crate::bindings::v4l2_ctrl_h264_pred_weights;
use crate::bindings::v4l2_ctrl_h264_scaling_matrix;
use crate::bindings::v4l2_ctrl_h264_slice_params;
use crate::bindings::v4l2_ctrl_h264_sps;
use crate::bindings::v4l2_ctrl_vp8_frame;
use crate::controls::ExtControlTrait;

bitflags! {
    /// FWHT Flags.
    #[derive(Clone, Copy, Debug)]
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

pub struct H264DecodeMode;
impl ExtControlTrait for H264DecodeMode {
    const ID: u32 = bindings::V4L2_CID_STATELESS_H264_DECODE_MODE;
    type PAYLOAD = i32;
}

pub struct H264StartCode;
impl ExtControlTrait for H264StartCode {
    const ID: u32 = bindings::V4L2_CID_STATELESS_H264_START_CODE;
    type PAYLOAD = i32;
}

pub struct H264Sps;
impl ExtControlTrait for H264Sps {
    const ID: u32 = bindings::V4L2_CID_STATELESS_H264_SPS;
    type PAYLOAD = v4l2_ctrl_h264_sps;
}

pub struct H264Pps;
impl ExtControlTrait for H264Pps {
    const ID: u32 = bindings::V4L2_CID_STATELESS_H264_PPS;
    type PAYLOAD = v4l2_ctrl_h264_pps;
}

pub struct H264ScalingMatrix;
impl ExtControlTrait for H264ScalingMatrix {
    const ID: u32 = bindings::V4L2_CID_STATELESS_H264_SCALING_MATRIX;
    type PAYLOAD = v4l2_ctrl_h264_scaling_matrix;
}

pub struct H264PredWeights;
impl ExtControlTrait for H264PredWeights {
    const ID: u32 = bindings::V4L2_CID_STATELESS_H264_PRED_WEIGHTS;
    type PAYLOAD = v4l2_ctrl_h264_pred_weights;
}

pub struct H264SliceParams;
impl ExtControlTrait for H264SliceParams {
    const ID: u32 = bindings::V4L2_CID_STATELESS_H264_SLICE_PARAMS;
    type PAYLOAD = v4l2_ctrl_h264_slice_params;
}

pub struct H264DecodeParams;
impl ExtControlTrait for H264DecodeParams {
    const ID: u32 = bindings::V4L2_CID_STATELESS_H264_DECODE_PARAMS;
    type PAYLOAD = v4l2_ctrl_h264_decode_params;
}

pub struct FwhtParams;
impl ExtControlTrait for FwhtParams {
    const ID: u32 = bindings::V4L2_CID_STATELESS_FWHT_PARAMS;
    type PAYLOAD = v4l2_ctrl_fwht_params;
}

bitflags! {
    /// VP8 Segment Flags.
    #[derive(Clone, Copy, Debug)]
    pub struct VP8SegmentFlags: u32 {
        const ENABLED = bindings::V4L2_VP8_SEGMENT_FLAG_ENABLED;
        const UPDATE_MAP = bindings::V4L2_VP8_SEGMENT_FLAG_UPDATE_MAP;
        const UPDATE_FEATURE_DATA = bindings::V4L2_VP8_SEGMENT_FLAG_UPDATE_FEATURE_DATA;
        const DELTA_VALUE_MODE = bindings::V4L2_VP8_SEGMENT_FLAG_DELTA_VALUE_MODE;
    }
}

bitflags! {
    /// VP8 Loop Filter Flags.
    #[derive(Clone, Copy, Debug)]
    pub struct VP8LoopFilterFlags: u32 {
        const ADJ_ENABLE = bindings::V4L2_VP8_LF_ADJ_ENABLE;
        const DELTA_UPDATE = bindings::V4L2_VP8_LF_DELTA_UPDATE;
        const FILTER_TYPE_SIMPLE = bindings::V4L2_VP8_LF_FILTER_TYPE_SIMPLE;
    }
}

bitflags! {
    /// VP8 Frame Flags.
    #[derive(Clone, Copy, Debug)]
    pub struct VP8FrameFlags: u32 {
        const KEY_FRAME = bindings::V4L2_VP8_FRAME_FLAG_KEY_FRAME;
        const EXPERIMENTAL = bindings::V4L2_VP8_FRAME_FLAG_EXPERIMENTAL;
        const SHOW_FRAME = bindings::V4L2_VP8_FRAME_FLAG_SHOW_FRAME;
        const NO_SKIP_COEFF = bindings::V4L2_VP8_FRAME_FLAG_MB_NO_SKIP_COEFF;
        const SIGN_BIAS_GOLDEN = bindings::V4L2_VP8_FRAME_FLAG_SIGN_BIAS_GOLDEN;
        const SIGN_BIAS_ALT  = bindings::V4L2_VP8_FRAME_FLAG_SIGN_BIAS_ALT;
    }
}

pub struct Vp8Frame;
impl ExtControlTrait for Vp8Frame {
    const ID: u32 = bindings::V4L2_CID_STATELESS_VP8_FRAME;
    type PAYLOAD = v4l2_ctrl_vp8_frame;
}

/// Safe wrapper over [`v4l2r::bindings::V4L2_CID_MPEG_VIDEO_HEADER_MODE`]
#[repr(i32)]
#[derive(N, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum VideoHeaderMode {
    Separate = bindings::v4l2_mpeg_video_header_mode_V4L2_MPEG_VIDEO_HEADER_MODE_SEPARATE as i32,
    JoinedWith1stFrame =
        bindings::v4l2_mpeg_video_header_mode_V4L2_MPEG_VIDEO_HEADER_MODE_JOINED_WITH_1ST_FRAME
            as i32,
}

impl ExtControlTrait for VideoHeaderMode {
    const ID: u32 = bindings::V4L2_CID_MPEG_VIDEO_HEADER_MODE;
    type PAYLOAD = i32;
}

impl From<VideoHeaderMode> for i32 {
    fn from(value: VideoHeaderMode) -> Self {
        value as i32
    }
}

/// Safe wrapper over [`bindings::V4L2_CID_MPEG_VIDEO_BITRATE`]
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct VideoBitrate(pub i32);

impl ExtControlTrait for VideoBitrate {
    const ID: u32 = bindings::V4L2_CID_MPEG_VIDEO_BITRATE;
    type PAYLOAD = i32;
}

impl From<VideoBitrate> for i32 {
    fn from(value: VideoBitrate) -> Self {
        value.0
    }
}

/// Safe wrapper over [`bindings::V4L2_CID_MPEG_VIDEO_BITRATE_PEAK`]
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct VideoBitratePeak(pub i32);

impl ExtControlTrait for VideoBitratePeak {
    const ID: u32 = bindings::V4L2_CID_MPEG_VIDEO_BITRATE_PEAK;
    type PAYLOAD = i32;
}

impl From<VideoBitratePeak> for i32 {
    fn from(value: VideoBitratePeak) -> Self {
        value.0
    }
}

/// Safe wrapper over [`bindings::V4L2_CID_MPEG_VIDEO_CONSTANT_QUALITY`]
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct VideoConstantQuality(pub i32);

impl ExtControlTrait for VideoConstantQuality {
    const ID: u32 = bindings::V4L2_CID_MPEG_VIDEO_CONSTANT_QUALITY;
    type PAYLOAD = i32;
}

impl From<VideoConstantQuality> for i32 {
    fn from(value: VideoConstantQuality) -> Self {
        value.0
    }
}

/// Safe wrapper over [`bindings::V4L2_CID_MPEG_VIDEO_BITRATE_MODE`]
#[repr(i32)]
#[derive(N, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum VideoBitrateMode {
    VariableBitrate =
        bindings::v4l2_mpeg_video_bitrate_mode_V4L2_MPEG_VIDEO_BITRATE_MODE_VBR as i32,
    ConstantBitrate =
        bindings::v4l2_mpeg_video_bitrate_mode_V4L2_MPEG_VIDEO_BITRATE_MODE_CBR as i32,
    ConstantQuality = bindings::v4l2_mpeg_video_bitrate_mode_V4L2_MPEG_VIDEO_BITRATE_MODE_CQ as i32,
}

impl ExtControlTrait for VideoBitrateMode {
    const ID: u32 = bindings::V4L2_CID_MPEG_VIDEO_BITRATE_MODE;
    type PAYLOAD = i32;
}

impl From<VideoBitrateMode> for i32 {
    fn from(value: VideoBitrateMode) -> Self {
        value as i32
    }
}

/// Safe wrapper over [`bindings::V4L2_CID_MPEG_VIDEO_FORCE_KEY_FRAME`]
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct VideoForceKeyFrame;

impl ExtControlTrait for VideoForceKeyFrame {
    const ID: u32 = bindings::V4L2_CID_MPEG_VIDEO_FORCE_KEY_FRAME;
    type PAYLOAD = i32;
}

impl From<VideoForceKeyFrame> for i32 {
    fn from(_value: VideoForceKeyFrame) -> Self {
        1
    }
}

/// Safe wrapper over [`bindings::V4L2_CID_MPEG_VIDEO_GOP_SIZE`]
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct VideoGopSize(pub i32);

impl ExtControlTrait for VideoGopSize {
    const ID: u32 = bindings::V4L2_CID_MPEG_VIDEO_GOP_SIZE;
    type PAYLOAD = i32;
}

impl From<VideoGopSize> for i32 {
    fn from(value: VideoGopSize) -> Self {
        value.0
    }
}

/// Safe wrapper over [`bindings::V4L2_CID_MPEG_VIDEO_B_FRAMES`]
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct VideoBFrames(pub i32);

impl ExtControlTrait for VideoBFrames {
    const ID: u32 = bindings::V4L2_CID_MPEG_VIDEO_B_FRAMES;
    type PAYLOAD = i32;
}

impl From<VideoBFrames> for i32 {
    fn from(value: VideoBFrames) -> Self {
        value.0
    }
}

/// Safe wrapper over [`bindings::V4L2_CID_MPEG_VIDEO_H264_MIN_QP`]
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct VideoH264MinQp(pub i32);

impl ExtControlTrait for VideoH264MinQp {
    const ID: u32 = bindings::V4L2_CID_MPEG_VIDEO_H264_MIN_QP;
    type PAYLOAD = i32;
}

impl From<VideoH264MinQp> for i32 {
    fn from(value: VideoH264MinQp) -> Self {
        value.0
    }
}

/// Safe wrapper over [`bindings::V4L2_CID_MPEG_VIDEO_H264_MAX_QP`]
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct VideoH264MaxQp(pub i32);

impl ExtControlTrait for VideoH264MaxQp {
    const ID: u32 = bindings::V4L2_CID_MPEG_VIDEO_H264_MAX_QP;
    type PAYLOAD = i32;
}

impl From<VideoH264MaxQp> for i32 {
    fn from(value: VideoH264MaxQp) -> Self {
        value.0
    }
}

/// Safe wrapper over [`bindings::V4L2_CID_MPEG_VIDEO_H264_I_PERIOD`]
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct VideoH264IPeriod(pub i32);

impl ExtControlTrait for VideoH264IPeriod {
    const ID: u32 = bindings::V4L2_CID_MPEG_VIDEO_H264_I_PERIOD;
    type PAYLOAD = i32;
}

impl From<VideoH264IPeriod> for i32 {
    fn from(value: VideoH264IPeriod) -> Self {
        value.0
    }
}

/// Safe wrapper over [`bindings::V4L2_CID_MPEG_VIDEO_H264_LEVEL`]
#[repr(i32)]
#[derive(N, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum VideoH264Level {
    L1_0 = bindings::v4l2_mpeg_video_h264_level_V4L2_MPEG_VIDEO_H264_LEVEL_1_0 as i32,
    L1B = bindings::v4l2_mpeg_video_h264_level_V4L2_MPEG_VIDEO_H264_LEVEL_1B as i32,
    L1_1 = bindings::v4l2_mpeg_video_h264_level_V4L2_MPEG_VIDEO_H264_LEVEL_1_1 as i32,
    L1_2 = bindings::v4l2_mpeg_video_h264_level_V4L2_MPEG_VIDEO_H264_LEVEL_1_2 as i32,
    L1_3 = bindings::v4l2_mpeg_video_h264_level_V4L2_MPEG_VIDEO_H264_LEVEL_1_3 as i32,
    L2_0 = bindings::v4l2_mpeg_video_h264_level_V4L2_MPEG_VIDEO_H264_LEVEL_2_0 as i32,
    L2_1 = bindings::v4l2_mpeg_video_h264_level_V4L2_MPEG_VIDEO_H264_LEVEL_2_1 as i32,
    L2_2 = bindings::v4l2_mpeg_video_h264_level_V4L2_MPEG_VIDEO_H264_LEVEL_2_2 as i32,
    L3_0 = bindings::v4l2_mpeg_video_h264_level_V4L2_MPEG_VIDEO_H264_LEVEL_3_0 as i32,
    L3_1 = bindings::v4l2_mpeg_video_h264_level_V4L2_MPEG_VIDEO_H264_LEVEL_3_1 as i32,
    L3_2 = bindings::v4l2_mpeg_video_h264_level_V4L2_MPEG_VIDEO_H264_LEVEL_3_2 as i32,
    L4_0 = bindings::v4l2_mpeg_video_h264_level_V4L2_MPEG_VIDEO_H264_LEVEL_4_0 as i32,
    L4_1 = bindings::v4l2_mpeg_video_h264_level_V4L2_MPEG_VIDEO_H264_LEVEL_4_1 as i32,
    L4_2 = bindings::v4l2_mpeg_video_h264_level_V4L2_MPEG_VIDEO_H264_LEVEL_4_2 as i32,
    L5_0 = bindings::v4l2_mpeg_video_h264_level_V4L2_MPEG_VIDEO_H264_LEVEL_5_0 as i32,
    L5_1 = bindings::v4l2_mpeg_video_h264_level_V4L2_MPEG_VIDEO_H264_LEVEL_5_1 as i32,
    L5_2 = bindings::v4l2_mpeg_video_h264_level_V4L2_MPEG_VIDEO_H264_LEVEL_5_2 as i32,
    L6_0 = bindings::v4l2_mpeg_video_h264_level_V4L2_MPEG_VIDEO_H264_LEVEL_6_0 as i32,
    L6_1 = bindings::v4l2_mpeg_video_h264_level_V4L2_MPEG_VIDEO_H264_LEVEL_6_1 as i32,
    L6_2 = bindings::v4l2_mpeg_video_h264_level_V4L2_MPEG_VIDEO_H264_LEVEL_6_2 as i32,
}

impl ExtControlTrait for VideoH264Level {
    const ID: u32 = bindings::V4L2_CID_MPEG_VIDEO_H264_LEVEL;
    type PAYLOAD = i32;
}

impl From<VideoH264Level> for i32 {
    fn from(value: VideoH264Level) -> Self {
        value as i32
    }
}

/// Safe wrapper over [`bindings::V4L2_CID_MPEG_VIDEO_H264_PROFILE`]
#[repr(i32)]
#[derive(N, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum VideoH264Profile {
    Baseline = bindings::v4l2_mpeg_video_h264_profile_V4L2_MPEG_VIDEO_H264_PROFILE_BASELINE as i32,
    ConstrainedBaseline =
        bindings::v4l2_mpeg_video_h264_profile_V4L2_MPEG_VIDEO_H264_PROFILE_CONSTRAINED_BASELINE
            as i32,
    Main = bindings::v4l2_mpeg_video_h264_profile_V4L2_MPEG_VIDEO_H264_PROFILE_MAIN as i32,
    Extended = bindings::v4l2_mpeg_video_h264_profile_V4L2_MPEG_VIDEO_H264_PROFILE_EXTENDED as i32,
    High = bindings::v4l2_mpeg_video_h264_profile_V4L2_MPEG_VIDEO_H264_PROFILE_HIGH as i32,
    High10 = bindings::v4l2_mpeg_video_h264_profile_V4L2_MPEG_VIDEO_H264_PROFILE_HIGH_10 as i32,
    High422 = bindings::v4l2_mpeg_video_h264_profile_V4L2_MPEG_VIDEO_H264_PROFILE_HIGH_422 as i32,
    High444Predictive =
        bindings::v4l2_mpeg_video_h264_profile_V4L2_MPEG_VIDEO_H264_PROFILE_HIGH_444_PREDICTIVE
            as i32,
    High10Intra =
        bindings::v4l2_mpeg_video_h264_profile_V4L2_MPEG_VIDEO_H264_PROFILE_HIGH_10_INTRA as i32,
    High422Intra =
        bindings::v4l2_mpeg_video_h264_profile_V4L2_MPEG_VIDEO_H264_PROFILE_HIGH_422_INTRA as i32,
    High444Intra =
        bindings::v4l2_mpeg_video_h264_profile_V4L2_MPEG_VIDEO_H264_PROFILE_HIGH_444_INTRA as i32,
    Cavlc444Intra =
        bindings::v4l2_mpeg_video_h264_profile_V4L2_MPEG_VIDEO_H264_PROFILE_CAVLC_444_INTRA as i32,
    ScalableBaseline =
        bindings::v4l2_mpeg_video_h264_profile_V4L2_MPEG_VIDEO_H264_PROFILE_SCALABLE_BASELINE
            as i32,
    ScalableHigh =
        bindings::v4l2_mpeg_video_h264_profile_V4L2_MPEG_VIDEO_H264_PROFILE_SCALABLE_HIGH as i32,
    ScalableHighIntra =
        bindings::v4l2_mpeg_video_h264_profile_V4L2_MPEG_VIDEO_H264_PROFILE_SCALABLE_HIGH_INTRA
            as i32,
    StereoHigh =
        bindings::v4l2_mpeg_video_h264_profile_V4L2_MPEG_VIDEO_H264_PROFILE_STEREO_HIGH as i32,
    MultiviewHigh =
        bindings::v4l2_mpeg_video_h264_profile_V4L2_MPEG_VIDEO_H264_PROFILE_MULTIVIEW_HIGH as i32,
    ConstrainedHigh =
        bindings::v4l2_mpeg_video_h264_profile_V4L2_MPEG_VIDEO_H264_PROFILE_CONSTRAINED_HIGH as i32,
}

impl ExtControlTrait for VideoH264Profile {
    const ID: u32 = bindings::V4L2_CID_MPEG_VIDEO_H264_PROFILE;
    type PAYLOAD = i32;
}

impl From<VideoH264Profile> for i32 {
    fn from(value: VideoH264Profile) -> Self {
        value as i32
    }
}

/// Safe wrapper over [`bindings::V4L2_CID_MPEG_VIDEO_PREPEND_SPSPPS_TO_IDR`]
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct VideoPrependSpsPpsToIdr(pub bool);

impl ExtControlTrait for VideoPrependSpsPpsToIdr {
    const ID: u32 = bindings::V4L2_CID_MPEG_VIDEO_PREPEND_SPSPPS_TO_IDR;
    type PAYLOAD = i32;
}

impl From<VideoPrependSpsPpsToIdr> for i32 {
    fn from(value: VideoPrependSpsPpsToIdr) -> Self {
        value.0 as i32
    }
}

/// Safe wrapper over [`bindings::V4L2_CID_MPEG_VIDEO_HEVC_MIN_QP`]
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct VideoHEVCMinQp(pub i32);

impl ExtControlTrait for VideoHEVCMinQp {
    const ID: u32 = bindings::V4L2_CID_MPEG_VIDEO_HEVC_MIN_QP;
    type PAYLOAD = i32;
}

impl From<VideoHEVCMinQp> for i32 {
    fn from(value: VideoHEVCMinQp) -> Self {
        value.0
    }
}

/// Safe wrapper over [`bindings::V4L2_CID_MPEG_VIDEO_HEVC_MAX_QP`]
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct VideoHEVCMaxQp(pub i32);

impl ExtControlTrait for VideoHEVCMaxQp {
    const ID: u32 = bindings::V4L2_CID_MPEG_VIDEO_HEVC_MAX_QP;
    type PAYLOAD = i32;
}

impl From<VideoHEVCMaxQp> for i32 {
    fn from(value: VideoHEVCMaxQp) -> Self {
        value.0
    }
}

/// Safe wrapper over [`bindings::V4L2_CID_MPEG_VIDEO_HEVC_LEVEL`]
#[repr(i32)]
#[derive(N, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum VideoHEVCLevel {
    L1_0 = bindings::v4l2_mpeg_video_hevc_level_V4L2_MPEG_VIDEO_HEVC_LEVEL_1 as i32,
    L2_0 = bindings::v4l2_mpeg_video_hevc_level_V4L2_MPEG_VIDEO_HEVC_LEVEL_2 as i32,
    L2_1 = bindings::v4l2_mpeg_video_hevc_level_V4L2_MPEG_VIDEO_HEVC_LEVEL_2_1 as i32,
    L3_0 = bindings::v4l2_mpeg_video_hevc_level_V4L2_MPEG_VIDEO_HEVC_LEVEL_3 as i32,
    L3_1 = bindings::v4l2_mpeg_video_hevc_level_V4L2_MPEG_VIDEO_HEVC_LEVEL_3_1 as i32,
    L4_0 = bindings::v4l2_mpeg_video_hevc_level_V4L2_MPEG_VIDEO_HEVC_LEVEL_4 as i32,
    L4_1 = bindings::v4l2_mpeg_video_hevc_level_V4L2_MPEG_VIDEO_HEVC_LEVEL_4_1 as i32,
    L5_0 = bindings::v4l2_mpeg_video_hevc_level_V4L2_MPEG_VIDEO_HEVC_LEVEL_5 as i32,
    L5_1 = bindings::v4l2_mpeg_video_hevc_level_V4L2_MPEG_VIDEO_HEVC_LEVEL_5_1 as i32,
    L5_2 = bindings::v4l2_mpeg_video_hevc_level_V4L2_MPEG_VIDEO_HEVC_LEVEL_5_2 as i32,
    L6_0 = bindings::v4l2_mpeg_video_hevc_level_V4L2_MPEG_VIDEO_HEVC_LEVEL_6 as i32,
    L6_1 = bindings::v4l2_mpeg_video_hevc_level_V4L2_MPEG_VIDEO_HEVC_LEVEL_6_1 as i32,
    L6_2 = bindings::v4l2_mpeg_video_hevc_level_V4L2_MPEG_VIDEO_HEVC_LEVEL_6_2 as i32,
}

impl ExtControlTrait for VideoHEVCLevel {
    const ID: u32 = bindings::V4L2_CID_MPEG_VIDEO_HEVC_LEVEL;
    type PAYLOAD = i32;
}

impl From<VideoHEVCLevel> for i32 {
    fn from(value: VideoHEVCLevel) -> Self {
        value as i32
    }
}

/// Safe wrapper over [`bindings::V4L2_CID_MPEG_VIDEO_HEVC_PROFILE`]
#[repr(i32)]
#[derive(N, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum VideoHEVCProfile {
    Main = bindings::v4l2_mpeg_video_hevc_profile_V4L2_MPEG_VIDEO_HEVC_PROFILE_MAIN as i32,
    Main10 = bindings::v4l2_mpeg_video_hevc_profile_V4L2_MPEG_VIDEO_HEVC_PROFILE_MAIN_10 as i32,
    MainStill =
        bindings::v4l2_mpeg_video_hevc_profile_V4L2_MPEG_VIDEO_HEVC_PROFILE_MAIN_STILL_PICTURE
            as i32,
}

impl ExtControlTrait for VideoHEVCProfile {
    const ID: u32 = bindings::V4L2_CID_MPEG_VIDEO_HEVC_PROFILE;
    type PAYLOAD = i32;
}

impl From<VideoHEVCProfile> for i32 {
    fn from(value: VideoHEVCProfile) -> Self {
        value as i32
    }
}

/// Safe wrapper over [`bindings::V4L2_CID_MPEG_VIDEO_VP8_PROFILE`]
#[repr(i32)]
#[derive(N, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum VideoVP8Profile {
    Profile0 = bindings::v4l2_mpeg_video_vp8_profile_V4L2_MPEG_VIDEO_VP8_PROFILE_0 as i32,
    Profile1 = bindings::v4l2_mpeg_video_vp8_profile_V4L2_MPEG_VIDEO_VP8_PROFILE_1 as i32,
    Profile2 = bindings::v4l2_mpeg_video_vp8_profile_V4L2_MPEG_VIDEO_VP8_PROFILE_2 as i32,
    Profile3 = bindings::v4l2_mpeg_video_vp8_profile_V4L2_MPEG_VIDEO_VP8_PROFILE_3 as i32,
}

impl ExtControlTrait for VideoVP8Profile {
    const ID: u32 = bindings::V4L2_CID_MPEG_VIDEO_VP8_PROFILE;
    type PAYLOAD = i32;
}

impl From<VideoVP8Profile> for i32 {
    fn from(value: VideoVP8Profile) -> Self {
        value as i32
    }
}

/// Safe wrapper over [`bindings::V4L2_CID_MPEG_VIDEO_VP9_PROFILE`]
#[repr(i32)]
#[derive(N, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum VideoVP9Profile {
    Profile0 = bindings::v4l2_mpeg_video_vp9_profile_V4L2_MPEG_VIDEO_VP9_PROFILE_0 as i32,
    Profile1 = bindings::v4l2_mpeg_video_vp9_profile_V4L2_MPEG_VIDEO_VP9_PROFILE_1 as i32,
    Profile2 = bindings::v4l2_mpeg_video_vp9_profile_V4L2_MPEG_VIDEO_VP9_PROFILE_2 as i32,
    Profile3 = bindings::v4l2_mpeg_video_vp9_profile_V4L2_MPEG_VIDEO_VP9_PROFILE_3 as i32,
}

impl ExtControlTrait for VideoVP9Profile {
    const ID: u32 = bindings::V4L2_CID_MPEG_VIDEO_VP9_PROFILE;
    type PAYLOAD = i32;
}

impl From<VideoVP9Profile> for i32 {
    fn from(value: VideoVP9Profile) -> Self {
        value as i32
    }
}

/// Safe wrapper over [`bindings::V4L2_CID_MPEG_VIDEO_VPX_MIN_QP`]
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct VideoVPXMinQp(pub i32);

impl ExtControlTrait for VideoVPXMinQp {
    const ID: u32 = bindings::V4L2_CID_MPEG_VIDEO_VPX_MIN_QP;
    type PAYLOAD = i32;
}

impl From<VideoVPXMinQp> for i32 {
    fn from(value: VideoVPXMinQp) -> Self {
        value.0
    }
}

/// Safe wrapper over [`bindings::V4L2_CID_MPEG_VIDEO_VPX_MAX_QP`]
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct VideoVPXMaxQp(pub i32);

impl ExtControlTrait for VideoVPXMaxQp {
    const ID: u32 = bindings::V4L2_CID_MPEG_VIDEO_VPX_MAX_QP;
    type PAYLOAD = i32;
}

impl From<VideoVPXMaxQp> for i32 {
    fn from(value: VideoVPXMaxQp) -> Self {
        value.0
    }
}

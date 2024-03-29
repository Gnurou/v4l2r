//! Definition of CODEC class controls.

use bitflags::bitflags;

use crate::bindings;
use crate::bindings::v4l2_ctrl_fwht_params;
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

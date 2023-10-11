//! Definition of USER class controls.

use crate::bindings;
use crate::controls::ExtControlTrait;

pub struct Brightness;
impl ExtControlTrait for Brightness {
    const ID: u32 = bindings::V4L2_CID_BRIGHTNESS;
    type PAYLOAD = i32;
}

pub struct Contrast;
impl ExtControlTrait for Contrast {
    const ID: u32 = bindings::V4L2_CID_CONTRAST;
    type PAYLOAD = i32;
}

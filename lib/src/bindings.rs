#![allow(dead_code)]
#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(deref_nullptr)]
#![allow(unaligned_references)]
#![allow(clippy::all)]

#[cfg(target_pointer_width = "64")]
include!("bindings/videodev2_64.rs");

#[cfg(target_pointer_width = "32")]
include!("bindings/videodev2_32.rs");

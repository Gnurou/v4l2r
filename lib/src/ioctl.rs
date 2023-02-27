//! Safer versions of the V4L2 ioctls through simple functions working on a
//! `RawFd` and Rust variants of the V4L2 structures. This module can be used
//! directly if that's the level of abstraction you are aiming for, but the
//! `device` module is very likely to be a better fit.
//!
//! ioctl functions of this module typically take the input parameters as
//! argument, and only return the values written by the kernel. Therefore,
//! although the return types look similar to the kernel structures, they are
//! not strictly identical.
mod decoder_cmd;
mod dqbuf;
mod encoder_cmd;
mod enum_fmt;
mod expbuf;
mod g_ext_ctrls;
mod g_fmt;
mod g_selection;
mod mmap;
mod qbuf;
mod querybuf;
mod querycap;
mod reqbufs;
mod request;
mod streamon;
mod subscribe_event;

pub use decoder_cmd::*;
pub use dqbuf::*;
pub use encoder_cmd::*;
pub use enum_fmt::*;
pub use expbuf::*;
pub use g_ext_ctrls::*;
pub use g_fmt::*;
pub use g_selection::*;
pub use mmap::*;
pub use qbuf::*;
pub use querybuf::*;
pub use querycap::*;
pub use reqbufs::*;
pub use request::*;
pub use streamon::*;
pub use subscribe_event::*;

use crate::bindings;
use crate::QueueType;
use std::{
    ffi::{CStr, FromBytesWithNulError},
    mem,
};

/// Utility function for sub-modules.
/// Constructs an owned String instance from a slice containing a nul-terminated
/// C string, after checking that the passed slice indeed contains a nul
/// character. The string is duplicated in the process so no ownership is taken
/// from the parameter.
fn string_from_cstr(c_str: &[u8]) -> Result<String, FromBytesWithNulError> {
    // Make sure that our string contains a nul character.
    let slice = match c_str.iter().position(|x| *x == b'\0') {
        // Pass an empty slice so from_bytes_with_nul returns an error.
        None => &[],
        Some(pos) => &c_str[..pos + 1],
    };

    Ok(CStr::from_bytes_with_nul(slice)?
        .to_string_lossy()
        .into_owned())
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_string_from_cstr() {
        use super::string_from_cstr;

        // Nul-terminated slice.
        assert_eq!(string_from_cstr(b"Hello\0"), Ok(String::from("Hello")));

        // Slice with nul in the middle and not nul-terminated.
        assert_eq!(string_from_cstr(b"Hi\0lo"), Ok(String::from("Hi")));

        // Slice with nul in the middle and nul-terminated.
        assert_eq!(string_from_cstr(b"Hi\0lo\0"), Ok(String::from("Hi")));

        // Slice starting with nul.
        assert_eq!(string_from_cstr(b"\0ello"), Ok(String::from("")));

        // Slice without nul.
        match string_from_cstr(b"Hello") {
            Err(_) => {}
            Ok(_) => panic!(),
        };

        // Empty slice.
        match string_from_cstr(b"") {
            Err(_) => {}
            Ok(_) => panic!(),
        };
    }
}

/// A memory area we can pass to ioctls in order to get/set plane information
/// with the multi-planar API.
type PlaneData = [bindings::v4l2_plane; bindings::VIDEO_MAX_PLANES as usize];

/// For simple initialization of `PlaneData`.
impl Default for bindings::v4l2_plane {
    fn default() -> Self {
        unsafe { mem::zeroed() }
    }
}

/// Returns whether the given queue type can handle multi-planar formats.
fn is_multi_planar(queue: QueueType) -> bool {
    matches!(
        queue,
        QueueType::VideoCaptureMplane | QueueType::VideoOutputMplane
    )
}

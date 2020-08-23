//! Provides safer versions of the V4L2 ioctls through simple functions working
//! on a `RawFd` and Rust variants of the V4L2 structures. This module can be
//! used directly if that's the level of abstraction you are aiming for, but
//! the `device` module is very likely to be a better fit.
//!
//! ioctl functions of this module typically take the input parameters as
//! argument, and only return the values written by the kernel. Therefore,
//! although the return types look similar to the kernel structures, they are
//! not strictly identical.
mod dqbuf;
mod encoder_cmd;
mod enum_fmt;
mod g_fmt;
mod mmap;
mod qbuf;
mod querybuf;
mod querycap;
mod reqbufs;
mod streamon;

pub use dqbuf::*;
pub use encoder_cmd::*;
pub use enum_fmt::*;
pub use g_fmt::*;
pub use mmap::*;
pub use qbuf::*;
pub use querybuf::*;
pub use querycap::*;
pub use reqbufs::*;
pub use streamon::*;

use crate::bindings;
use crate::QueueType;
use crate::Result;
use std::ffi::CStr;

/// Utility function for sub-modules.
/// Constructs an owned String instance from a slice containing a nul-terminated
/// C string, after checking that the passed slice indeed contains a nul
/// character. The string is duplicated in the process so no ownership is taken
/// from the parameter.
fn string_from_cstr(c_str: &[u8]) -> Result<String> {
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
        use crate::Error::*;

        // Nul-terminated slice.
        assert_eq!(string_from_cstr(b"Hello\0"), Ok(String::from("Hello")));

        // Slice with nul in the middle and not nul-terminated.
        assert_eq!(string_from_cstr(b"Hi\0lo"), Ok(String::from("Hi")));

        // Slice with nul in the middle and nul-terminated.
        assert_eq!(string_from_cstr(b"Hi\0lo\0"), Ok(String::from("Hi")));

        // Slice starting with nul.
        assert_eq!(string_from_cstr(b"\0ello"), Ok(String::from("")));

        // Slice without nul.
        match string_from_cstr(b"Hello").unwrap_err() {
            FfiInvalidString(_) => {}
            _ => panic!(),
        };

        // Empty slice.
        match string_from_cstr(b"").unwrap_err() {
            FfiInvalidString(_) => {}
            _ => panic!(),
        };
    }
}

/// A memory area we can pass to ioctls in order to get/set plane information
/// with the multi-planar API.
type PlaneData = [bindings::v4l2_plane; bindings::VIDEO_MAX_PLANES as usize];

/// Returns whether the given queue type can handle multi-planar formats.
fn is_multi_planar(queue: QueueType) -> bool {
    match queue {
        QueueType::VideoCaptureMplane | QueueType::VideoOutputMplane => true,
        _ => false,
    }
}

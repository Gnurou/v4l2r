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
mod g_input;
mod g_selection;
mod mmap;
mod qbuf;
mod querybuf;
mod querycap;
mod queryctrl;
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
pub use g_input::*;
pub use g_selection::*;
pub use mmap::*;
use nix::errno::Errno;
pub use qbuf::*;
pub use querybuf::*;
pub use querycap::*;
pub use queryctrl::*;
pub use reqbufs::*;
pub use request::*;
pub use streamon::*;
pub use subscribe_event::*;

use std::fmt::Debug;

use crate::bindings;
use crate::QueueType;
use std::{
    ffi::{CStr, FromBytesWithNulError},
    mem,
};

/// Utility function for sub-modules.
/// Constructs an owned String instance from a slice containing a nul-terminated
/// C string, after checking that the passed slice indeed contains a nul
/// character.
fn string_from_cstr(c_str: &[u8]) -> Result<String, FromBytesWithNulError> {
    // Make sure that our string contains a nul character.
    let slice = match c_str.iter().position(|x| *x == b'\0') {
        // Pass the full slice, `from_bytes_with_nul` will return an error.
        None => c_str,
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

/// Returns whether the given queue type can handle multi-planar formats.
fn is_multi_planar(queue: QueueType) -> bool {
    matches!(
        queue,
        QueueType::VideoCaptureMplane | QueueType::VideoOutputMplane
    )
}

/// Extension trait for allowing easy conversion of ioctl errors into their originating error code.
pub trait IntoErrno {
    fn into_errno(self) -> i32;
}

impl<T> IntoErrno for T
where
    T: Into<Errno>,
{
    fn into_errno(self) -> i32 {
        self.into() as i32
    }
}

/// A memory area we can pass to ioctls in order to get/set plane information
/// with the multi-planar API.
type V4l2BufferPlanes = [bindings::v4l2_plane; bindings::VIDEO_MAX_PLANES as usize];

/// For simple initialization of `V4l2BufferPlanes`.
impl Default for bindings::v4l2_plane {
    fn default() -> Self {
        unsafe { mem::zeroed() }
    }
}

/// Information about a single plane of a V4L2 buffer.
pub struct V4l2BufferPlane<'a> {
    plane: &'a bindings::v4l2_plane,
}

impl<'a> Debug for V4l2BufferPlane<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("V4l2BufferPlane")
            .field("length", &self.length())
            .field("bytesused", &self.bytesused())
            .field("data_offset", &self.data_offset())
            .finish()
    }
}

impl<'a> V4l2BufferPlane<'a> {
    pub fn length(&self) -> u32 {
        self.plane.length
    }

    pub fn bytesused(&self) -> u32 {
        self.plane.bytesused
    }

    pub fn data_offset(&self) -> u32 {
        self.plane.data_offset
    }
}

/// A completely owned v4l2_buffer, where the pointer to planes is meaningless and fixed up when
/// needed.
///
/// If the buffer is single-planar, it is modified to use `planes` anyway for the information of
/// its unique plane.
#[derive(Clone)]
#[repr(C)]
pub struct V4l2Buffer {
    buffer: bindings::v4l2_buffer,
    planes: V4l2BufferPlanes,
}

/// V4l2Buffer is safe to send across threads. `v4l2_buffer` is !Send because it
/// contains a pointer, but we are making sure to use it safely here.
unsafe impl Send for V4l2Buffer {}

impl Debug for V4l2Buffer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("V4l2Buffer")
            .field("index", &self.index())
            .field("flags", &self.flags())
            .field("sequence", &self.sequence())
            .finish()
    }
}

impl V4l2Buffer {
    pub fn index(&self) -> u32 {
        self.buffer.index
    }

    pub fn queue_type(&self) -> u32 {
        self.buffer.type_
    }

    pub fn flags(&self) -> BufferFlags {
        BufferFlags::from_bits_truncate(self.buffer.flags)
    }

    pub fn is_last(&self) -> bool {
        self.flags().contains(BufferFlags::LAST)
    }

    pub fn timestamp(&self) -> bindings::timeval {
        self.buffer.timestamp
    }

    pub fn sequence(&self) -> u32 {
        self.buffer.sequence
    }

    pub fn is_multi_planar(&self) -> bool {
        matches!(
            self.buffer.type_,
            bindings::v4l2_buf_type_V4L2_BUF_TYPE_VIDEO_OUTPUT_MPLANE
                | bindings::v4l2_buf_type_V4L2_BUF_TYPE_VIDEO_CAPTURE_MPLANE
        )
    }

    pub fn num_planes(&self) -> usize {
        if self.is_multi_planar() {
            self.buffer.length as usize
        } else {
            1
        }
    }

    /// Returns the first plane of the buffer. This method is guaranteed to
    /// succeed because every buffer has at least one plane.
    pub fn get_first_plane(&self) -> V4l2BufferPlane {
        V4l2BufferPlane {
            plane: &self.planes[0],
        }
    }

    /// Returns plane `index` of the buffer, or `None` if `index` is larger than
    /// the number of planes in this buffer.
    pub fn get_plane(&self, index: usize) -> Option<V4l2BufferPlane> {
        if index < self.num_planes() {
            Some(V4l2BufferPlane {
                plane: &self.planes[index],
            })
        } else {
            None
        }
    }

    /// Returns the raw v4l2_buffer as a pointer. Useful to pass to unsafe
    /// non-Rust code.
    pub fn as_raw_v4l2_buffer(&self) -> *const bindings::v4l2_buffer {
        &self.buffer
    }
}

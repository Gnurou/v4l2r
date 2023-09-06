//! Provides safer versions of the V4L2 ioctls through simple functions working on a `RawFd`, and
//! safer variants of the main V4L2 structures. This module can be used directly, but the `device`
//! module is very likely to be a better fit for application code.
//!
//! V4L2 ioctls are usually called with a single structure as argument, which serves to store both
//! the input and output of the ioctl. This tend to be error prone.
//!
//! Consequently, each ioctl proxy function is designed as follows:
//!
//! * The parameters of the function correspond to the input values of the ioctl.
//! * The returned value of the function is a value that can be constructed from the ioctl's
//! structure parameter (i.e. it can be the parameter itself)
//!
//! For instance, the `VIDIOC_G_FMT` ioctl takes a `struct v4l2_format` as argument, but only the
//! its `type` field is set by user-space - the rest of the structure is to be filled by the
//! driver.
//!
//! Therefore, our `[g_fmt]` ioctl proxy function takes the requested queue type as argument and
//! takes care of managing the `struct v4l2_format` to be passed to the kernel. The filled
//! structure is then converted into the type desired by the caller using `TryFrom<v4l2_format>`:
//!
//! ```text
//! pub fn g_fmt<O: TryFrom<bindings::v4l2_format>>(
//!     fd: &impl AsRawFd,
//!     queue: QueueType,
//! ) -> Result<O, GFmtError>;
//! ```
//!
//! Since `struct v4l2_format` has C unions that are unsafe to use in Rust, the `[Format]` type can
//! be used as the output type of this function, to validate the `struct v4l2_format` returned by
//! the kernel and convert it to a safe type.
//!
//! Most ioctls also have their own error type: this helps discern scenarios where the ioctl
//! returned non-zero, but the situation is not necessarily an error. For instance, `VIDIOC_DQBUF`
//! can return -EAGAIN if no buffer is available to dequeue, which is not an error and thus is
//! represented by its own variant. Actual errors are captured by the `IoctlError` variant, and all
//! error types can be converted to their original error code using their `Into<Errno>`
//! implementation.
//!
//! All the ioctls of this module are implemented following this model, which should be safer to
//! use and less prone to user errors.

mod decoder_cmd;
mod dqbuf;
mod encoder_cmd;
mod enum_fmt;
mod expbuf;
mod frameintervals;
mod framesizes;
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
pub use frameintervals::*;
pub use framesizes::*;
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
use crate::memory::MemoryType;
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
    buffer: &'a bindings::v4l2_buffer,
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

    /// Returns the memory offset of this plane if the buffer's type is MMAP.
    pub fn mem_offset(&self) -> Option<u32> {
        if MemoryType::n(self.buffer.memory) == Some(MemoryType::Mmap) {
            // Safe because we are returning a u32 in any case. It may be garbage, but will just
            // lead to a runtime error down the road.
            // Additionally we checked that the memory type of the buffer was MMAP, so the value
            // should be valid.
            Some(unsafe { self.plane.m.mem_offset })
        } else {
            None
        }
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

    pub fn queue_type(&self) -> QueueType {
        QueueType::n(self.buffer.type_).unwrap()
    }

    pub fn memory(&self) -> MemoryType {
        MemoryType::n(self.buffer.memory).unwrap()
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
            buffer: &self.buffer,
            plane: &self.planes[0],
        }
    }

    /// Returns plane `index` of the buffer, or `None` if `index` is larger than
    /// the number of planes in this buffer.
    pub fn get_plane(&self, index: usize) -> Option<V4l2BufferPlane> {
        if index < self.num_planes() {
            Some(V4l2BufferPlane {
                buffer: &self.buffer,
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

    /// Returns a reference to the internal `v4l2_buffer`. All pointers in this
    /// structure are invalid.
    pub fn v4l2_buffer(&self) -> &bindings::v4l2_buffer {
        &self.buffer
    }

    /// Returns an iterator to the internal `v4l2_plane`s. All pointers in the
    /// planes are invalid.
    pub fn v4l2_plane_iter(&self) -> impl Iterator<Item = &bindings::v4l2_plane> {
        self.planes.iter()
    }
}

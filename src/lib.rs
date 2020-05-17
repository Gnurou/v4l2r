//! This library provides two levels of abstraction over V4L2:
//!
//! * The `ioctl` module provides direct, thin wrappers over the V4L2 ioctls
//!   with added safety. Note that "safety" here is in terms of memory safety:
//!   this layer won't guard against passing invalid data that the ioctls will
//!   reject - it just makes sure that data passed from and to the kernel can
//!   be accessed safely. Since this is a 1:1 mapping over the V4L2 ioctls,
//!   working at this level is a bit laborious, although more comfortable than
//!   doing the same in C.
//!
//! * The `device` module (still WIP) provides a higher-level abstraction over
//!   the V4L2 entities, like device and queue. Strong typing will ensure that
//!   most inconsistencies while using the V4L2 API can be caught at
//!   compile-time.
//!
//! These two layers should provide the foundations for higher-level libraries
//! to provide safe, specialized APIs that support various V4L2 usage scenarios
//! (camera, decoder/encoder, etc).
//!
mod bindings;
pub mod ioctl;
pub mod memory;

use std::ffi;
use std::fmt;
use std::fmt::{Debug, Display};

// The goal of this library is to provide two layers of abstraction:
// ioctl: direct, safe counterparts of the V4L2 ioctls.
// device/queue/buffer: higher abstraction, still mapping to core V4L2 mechanics.

/// Error type for anything wrong that can happen within V4L2.
#[derive(Debug, PartialEq)]
pub enum Error {
    /// The requested item is already in use by the client.
    AlreadyBorrowed,
    /// The buffer information provided is of the wrong memory type.
    WrongMemoryType,
    /// A v4l2_format cannot be converted to the desired format type because its
    /// type member does not match.
    InvalidBufferType,
    /// A request to queue buffers has been done, but it did not contain enough
    /// plane descriptors.
    NotEnoughPlanes,
    /// A v4l2_format pretends it has more planes that it can possibly contain,
    /// or too many planes provided when queing buffer.
    TooManyPlanes,
    /// A non-zero data_offset has been specified for a plane while using the
    /// single-planar API, which does not support this parameter.
    DataOffsetNotSupported,
    /// Buffer does not exist, either we have requested a buffer index that does
    /// not exist, or we try to submit a buffer that has been deleted while we
    /// were preparing it.
    InvalidBuffer,
    Nix(nix::Error),
    FfiNul(ffi::NulError),
    FfiInvalidString(ffi::FromBytesWithNulError),
}

impl Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::AlreadyBorrowed => write!(f, "Already in use"),
            Error::WrongMemoryType => write!(f, "Wrong memory type"),
            Error::InvalidBufferType => write!(f, "Invalid buffer type"),
            Error::NotEnoughPlanes => write!(f, "Not enough planes specified"),
            Error::TooManyPlanes => write!(f, "Too many planes specified"),
            Error::DataOffsetNotSupported => write!(f, "Data offset not supported"),
            Error::InvalidBuffer => write!(f, "Invalid buffer"),
            Error::Nix(e) => Debug::fmt(e, f),
            Error::FfiNul(e) => Debug::fmt(e, f),
            Error::FfiInvalidString(e) => Debug::fmt(e, f),
        }
    }
}
impl std::error::Error for Error {}

impl From<nix::Error> for Error {
    fn from(e: nix::Error) -> Self {
        Error::Nix(e)
    }
}

impl From<ffi::NulError> for Error {
    fn from(e: ffi::NulError) -> Self {
        Error::FfiNul(e)
    }
}

impl From<ffi::FromBytesWithNulError> for Error {
    fn from(e: ffi::FromBytesWithNulError) -> Self {
        Error::FfiInvalidString(e)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// Types of queues currently supported by this library.
#[allow(unused)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum QueueType {
    VideoCapture = bindings::v4l2_buf_type_V4L2_BUF_TYPE_VIDEO_CAPTURE as isize,
    VideoOutput = bindings::v4l2_buf_type_V4L2_BUF_TYPE_VIDEO_OUTPUT as isize,
    VideoCaptureMplane = bindings::v4l2_buf_type_V4L2_BUF_TYPE_VIDEO_CAPTURE_MPLANE as isize,
    VideoOutputMplane = bindings::v4l2_buf_type_V4L2_BUF_TYPE_VIDEO_OUTPUT_MPLANE as isize,
}

mod pixel_format {
    use std::fmt;

    /// A Fourcc pixel format, used to pass formats to V4L2. It can be converted
    /// back and forth from a 32-bit integer, or a 4-bytes string.
    #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
    pub struct PixelFormat(u32);

    /// Convert a Fourcc in 32-bit integer format (like the ones passed in V4L2
    /// structures) into the matching pixel format.
    impl From<u32> for PixelFormat {
        fn from(i: u32) -> Self {
            PixelFormat(i)
        }
    }

    /// Convert a pixel format back to its 32-bit representation.
    impl From<PixelFormat> for u32 {
        fn from(format: PixelFormat) -> Self {
            format.0
        }
    }

    /// Simple way to convert a string litteral (e.g. b"NV12") into a pixel
    /// format that can be passed to V4L2.
    impl From<&[u8; 4]> for PixelFormat {
        fn from(n: &[u8; 4]) -> Self {
            PixelFormat(
                n[0] as u32 | (n[1] as u32) << 8 | (n[2] as u32) << 16 | (n[3] as u32) << 24,
            )
        }
    }

    /// Convert a pixel format back to its 4-character representation.
    impl From<PixelFormat> for [u8; 4] {
        fn from(format: PixelFormat) -> Self {
            format.0.to_le_bytes()
        }
    }

    impl fmt::Debug for PixelFormat {
        fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.write_fmt(format_args!("0x{:08x} ({})", self.0, self))
        }
    }

    impl fmt::Display for PixelFormat {
        fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
            let fourcc = self
                .0
                .to_le_bytes()
                .iter()
                .map(|&x| x as char)
                .collect::<String>();
            f.write_str(fourcc.as_str())
        }
    }

    #[cfg(test)]
    mod tests {
        use super::PixelFormat;

        // "NV12"
        const NV12_U32: u32 = u32::from_le(0x3231564e);
        const NV12_U8S: &[u8; 4] = b"NV12";
        const NV12_STRING: &str = "NV12";

        #[test]
        fn pixelformat_from_u32() {
            let format = PixelFormat::from(NV12_U32);
            let to_u32: u32 = format.into();
            let to_u8s: [u8; 4] = format.into();
            let to_string = format.to_string();
            assert_eq!(to_u32, NV12_U32);
            assert_eq!(&to_u8s, NV12_U8S);
            assert_eq!(to_string, NV12_STRING);
        }

        #[test]
        fn pixelformat_from_u8s() {
            let format = PixelFormat::from(NV12_U8S);
            let to_u32: u32 = format.into();
            let to_u8s: [u8; 4] = format.into();
            let to_string = format.to_string();
            assert_eq!(to_u32, NV12_U32);
            assert_eq!(&to_u8s, NV12_U8S);
            assert_eq!(to_string, NV12_STRING);
        }
    }
}
pub use pixel_format::*;

mod format {
    use super::PixelFormat;

    #[derive(Debug, PartialEq, Clone, Default)]
    pub struct PlanePixFormat {
        pub sizeimage: u32,
        pub bytesperline: u32,
    }

    /// Unified representation of a V4L2 format capable of handling both single
    /// and multi-planar formats. When the single-planar API is used, only
    /// one plane shall be used - attempts to have more will be rejected by the
    /// ioctl wrappers.
    #[derive(Debug, PartialEq, Clone, Default)]
    pub struct Format {
        pub width: u32,
        pub height: u32,
        pub pixelformat: PixelFormat,
        pub plane_fmt: Vec<PlanePixFormat>,
    }

    /// Quickly build a usable `Format` from a pixel format and resolution.
    impl<T: Into<PixelFormat>> From<(T, (usize, usize))> for Format {
        fn from((pixel_format, (width, height)): (T, (usize, usize))) -> Self {
            Format {
                width: width as u32,
                height: height as u32,
                pixelformat: pixel_format.into(),
                ..Default::default()
            }
        }
    }
}
pub use format::*;

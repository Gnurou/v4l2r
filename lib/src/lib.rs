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
#[doc(hidden)]
pub mod bindings;
pub mod decoder;
pub mod device;
pub mod encoder;
pub mod ioctl;
pub mod memory;

use std::convert::TryFrom;
use std::fmt;
use std::fmt::{Debug, Display};

use thiserror::Error;

// The goal of this library is to provide two layers of abstraction:
// ioctl: direct, safe counterparts of the V4L2 ioctls.
// device/queue/buffer: higher abstraction, still mapping to core V4L2 mechanics.

/// Types of queues currently supported by this library.
#[allow(unused)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum QueueType {
    VideoCapture = bindings::v4l2_buf_type_V4L2_BUF_TYPE_VIDEO_CAPTURE as isize,
    VideoOutput = bindings::v4l2_buf_type_V4L2_BUF_TYPE_VIDEO_OUTPUT as isize,
    VideoCaptureMplane = bindings::v4l2_buf_type_V4L2_BUF_TYPE_VIDEO_CAPTURE_MPLANE as isize,
    VideoOutputMplane = bindings::v4l2_buf_type_V4L2_BUF_TYPE_VIDEO_OUTPUT_MPLANE as isize,
}

impl Display for QueueType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        Debug::fmt(self, f)
    }
}

/// A Fourcc pixel format, used to pass formats to V4L2. It can be converted
/// back and forth from a 32-bit integer, or a 4-bytes string.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct PixelFormat(u32);

/// Converts a Fourcc in 32-bit integer format (like the ones passed in V4L2
/// structures) into the matching pixel format.
///
/// # Examples
///
/// ```
/// # use v4l2r::PixelFormat;
/// // Fourcc representation of NV12.
/// let nv12 = u32::from_le(0x3231564e);
/// let f = PixelFormat::from(nv12);
/// assert_eq!(u32::from(f), nv12);
/// ```
impl From<u32> for PixelFormat {
    fn from(i: u32) -> Self {
        PixelFormat(i)
    }
}

/// Converts a pixel format back to its 32-bit representation.
///
/// # Examples
///
/// ```
/// # use v4l2r::PixelFormat;
/// // Fourcc representation of NV12.
/// let nv12 = u32::from_le(0x3231564e);
/// let f = PixelFormat::from(nv12);
/// assert_eq!(u32::from(f), nv12);
/// ```
impl From<PixelFormat> for u32 {
    fn from(format: PixelFormat) -> Self {
        format.0
    }
}

/// Simple way to convert a string litteral (e.g. b"NV12") into a pixel
/// format that can be passed to V4L2.
///
/// # Examples
///
/// ```
/// # use v4l2r::PixelFormat;
/// let nv12 = b"NV12";
/// let f = PixelFormat::from(nv12);
/// assert_eq!(&<[u8; 4]>::from(f), nv12);
/// ```
impl From<&[u8; 4]> for PixelFormat {
    fn from(n: &[u8; 4]) -> Self {
        PixelFormat(n[0] as u32 | (n[1] as u32) << 8 | (n[2] as u32) << 16 | (n[3] as u32) << 24)
    }
}

/// Convert a pixel format back to its 4-character representation.
///
/// # Examples
///
/// ```
/// # use v4l2r::PixelFormat;
/// let nv12 = b"NV12";
/// let f = PixelFormat::from(nv12);
/// assert_eq!(&<[u8; 4]>::from(f), nv12);
/// ```
impl From<PixelFormat> for [u8; 4] {
    fn from(format: PixelFormat) -> Self {
        format.0.to_le_bytes()
    }
}

/// Produces a debug string for this PixelFormat, including its hexadecimal
/// and string representation.
///
/// # Examples
///
/// ```
/// # use v4l2r::PixelFormat;
/// // Fourcc representation of NV12.
/// let nv12 = u32::from_le(0x3231564e);
/// let f = PixelFormat::from(nv12);
/// assert_eq!(format!("{:?}", f), "0x3231564e (NV12)");
/// ```
impl fmt::Debug for PixelFormat {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_fmt(format_args!("0x{:08x} ({})", self.0, self))
    }
}

/// Produces a displayable form of this PixelFormat.
///
/// # Examples
///
/// ```
/// # use v4l2r::PixelFormat;
/// // Fourcc representation of NV12.
/// let nv12 = u32::from_le(0x3231564e);
/// let f = PixelFormat::from(nv12);
/// assert_eq!(f.to_string(), "NV12");
/// ```
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

/// Description of a single plane in a format.
#[derive(Debug, PartialEq, Clone, Default)]
pub struct PlaneLayout {
    /// Useful size of the plane ; the backing memory must be at least that large.
    pub sizeimage: u32,
    /// Bytes per line of data. Only meaningful for image formats.
    pub bytesperline: u32,
}

/// Unified representation of a V4L2 format capable of handling both single
/// and multi-planar formats. When the single-planar API is used, only
/// one plane shall be used - attempts to have more will be rejected by the
/// ioctl wrappers.
#[derive(Debug, PartialEq, Clone, Default)]
pub struct Format {
    /// Width of the image in pixels.
    pub width: u32,
    /// Height of the image in pixels.
    pub height: u32,
    /// Format each pixel is encoded in.
    pub pixelformat: PixelFormat,
    /// Individual layout of each plane in this format. The exact number of planes
    /// is defined by `pixelformat`.
    pub plane_fmt: Vec<PlaneLayout>,
}

#[derive(Debug, Error, PartialEq)]
pub enum FormatConversionError {
    #[error("Too many planes ({0}) specified,")]
    TooManyPlanes(usize),
    #[error("Invalid buffer type requested")]
    InvalidBufferType(u32),
}

impl TryFrom<bindings::v4l2_format> for Format {
    type Error = FormatConversionError;

    fn try_from(fmt: bindings::v4l2_format) -> std::result::Result<Self, Self::Error> {
        match fmt.type_ {
            bindings::v4l2_buf_type_V4L2_BUF_TYPE_VIDEO_CAPTURE
            | bindings::v4l2_buf_type_V4L2_BUF_TYPE_VIDEO_OUTPUT => {
                let pix = unsafe { &fmt.fmt.pix };
                Ok(Format {
                    width: pix.width,
                    height: pix.height,
                    pixelformat: PixelFormat::from(pix.pixelformat),
                    plane_fmt: vec![PlaneLayout {
                        bytesperline: pix.bytesperline,
                        sizeimage: pix.sizeimage,
                    }],
                })
            }
            bindings::v4l2_buf_type_V4L2_BUF_TYPE_VIDEO_CAPTURE_MPLANE
            | bindings::v4l2_buf_type_V4L2_BUF_TYPE_VIDEO_OUTPUT_MPLANE => {
                let pix_mp = unsafe { &fmt.fmt.pix_mp };

                // Can only happen if we passed a malformed v4l2_format.
                if pix_mp.num_planes as usize > pix_mp.plane_fmt.len() {
                    return Err(Self::Error::TooManyPlanes(pix_mp.num_planes as usize));
                }

                let mut plane_fmt = Vec::new();
                for i in 0..pix_mp.num_planes as usize {
                    let plane = &pix_mp.plane_fmt[i];
                    plane_fmt.push(PlaneLayout {
                        sizeimage: plane.sizeimage,
                        bytesperline: plane.bytesperline,
                    });
                }

                Ok(Format {
                    width: pix_mp.width,
                    height: pix_mp.height,
                    pixelformat: PixelFormat::from(pix_mp.pixelformat),
                    plane_fmt,
                })
            }
            t => Err(Self::Error::InvalidBufferType(t)),
        }
    }
}

/// Quickly build a usable `Format` from a pixel format and resolution.
///
/// # Examples
///
/// ```
/// # use v4l2r::Format;
/// let f = Format::from((b"NV12", (640, 480)));
/// assert_eq!(f.width, 640);
/// assert_eq!(f.height, 480);
/// assert_eq!(f.pixelformat.to_string(), "NV12");
/// assert_eq!(f.plane_fmt.len(), 0);
/// ```
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

/// A more elegant representation for `v4l2_rect`.
#[derive(Debug)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub width: u32,
    pub height: u32,
}

impl Rect {
    pub fn new(left: i32, top: i32, width: u32, height: u32) -> Rect {
        Rect {
            left,
            top,
            width,
            height,
        }
    }
}

impl From<bindings::v4l2_rect> for Rect {
    fn from(rect: bindings::v4l2_rect) -> Self {
        Rect {
            left: rect.left,
            top: rect.top,
            width: rect.width,
            height: rect.height,
        }
    }
}

impl From<Rect> for bindings::v4l2_rect {
    fn from(rect: Rect) -> Self {
        bindings::v4l2_rect {
            left: rect.left,
            top: rect.top,
            width: rect.width,
            height: rect.height,
        }
    }
}

impl Display for Rect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "({}, {}), {}x{}",
            self.left, self.top, self.width, self.height
        )
    }
}

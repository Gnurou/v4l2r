//! Safe wrapper for the `VIDIOC_(G|S|TRY)_FMT` ioctls.
use crate::bindings;
use crate::{Format, PixelFormat, PlaneLayout, QueueType};
use nix::errno::Errno;
use nix::Error;
use std::convert::{From, Into, TryFrom, TryInto};
use std::default::Default;
use std::mem;
use std::os::unix::io::AsRawFd;
use thiserror::Error;

#[derive(Debug, Error, PartialEq)]
pub enum FormatConversionError {
    #[error("Too many planes ({0}) specified,")]
    TooManyPlanes(usize),
    #[error("Invalid buffer type requested")]
    InvalidBufferType(u32),
}

/// Implementors can receive the result from the `g_fmt`, `s_fmt` and `try_fmt`
/// ioctls.
pub trait Fmt<E: Into<FormatConversionError>>: TryFrom<bindings::v4l2_format, Error = E> {}

impl Into<FormatConversionError> for std::convert::Infallible {
    fn into(self) -> FormatConversionError {
        FormatConversionError::TooManyPlanes(0)
    }
}

impl Fmt<std::convert::Infallible> for bindings::v4l2_format {}

impl TryFrom<(Format, QueueType)> for bindings::v4l2_format {
    type Error = FormatConversionError;

    fn try_from((format, queue): (Format, QueueType)) -> Result<Self, Self::Error> {
        Ok(bindings::v4l2_format {
            type_: queue as u32,
            fmt: match queue {
                QueueType::VideoCaptureMplane | QueueType::VideoOutputMplane => {
                    bindings::v4l2_format__bindgen_ty_1 {
                        pix_mp: {
                            if format.plane_fmt.len() > bindings::VIDEO_MAX_PLANES as usize {
                                return Err(Self::Error::TooManyPlanes(format.plane_fmt.len()));
                            }

                            let mut pix_mp = bindings::v4l2_pix_format_mplane {
                                width: format.width,
                                height: format.height,
                                pixelformat: format.pixelformat.into(),
                                num_planes: format.plane_fmt.len() as u8,
                                plane_fmt: Default::default(),
                                ..unsafe { mem::zeroed() }
                            };

                            for (plane, v4l2_plane) in format
                                .plane_fmt
                                .into_iter()
                                .zip(pix_mp.plane_fmt.iter_mut())
                            {
                                *v4l2_plane = plane.into();
                            }

                            pix_mp
                        },
                    }
                }
                _ => bindings::v4l2_format__bindgen_ty_1 {
                    pix: {
                        if format.plane_fmt.len() > 1 {
                            return Err(Self::Error::TooManyPlanes(format.plane_fmt.len()));
                        }

                        let (bytesperline, sizeimage) = if !format.plane_fmt.is_empty() {
                            (
                                format.plane_fmt[0].bytesperline,
                                format.plane_fmt[0].sizeimage,
                            )
                        } else {
                            Default::default()
                        };

                        bindings::v4l2_pix_format {
                            width: format.width,
                            height: format.height,
                            pixelformat: format.pixelformat.into(),
                            bytesperline,
                            sizeimage,
                            ..unsafe { mem::zeroed() }
                        }
                    },
                },
            },
        })
    }
}

impl TryFrom<bindings::v4l2_format> for Format {
    type Error = FormatConversionError;

    fn try_from(fmt: bindings::v4l2_format) -> Result<Self, Self::Error> {
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

impl Fmt<FormatConversionError> for Format {}

impl Default for bindings::v4l2_plane_pix_format {
    fn default() -> Self {
        bindings::v4l2_plane_pix_format {
            sizeimage: 0,
            bytesperline: 0,
            reserved: Default::default(),
        }
    }
}

impl From<PlaneLayout> for bindings::v4l2_plane_pix_format {
    fn from(plane: PlaneLayout) -> Self {
        bindings::v4l2_plane_pix_format {
            sizeimage: plane.sizeimage,
            bytesperline: plane.bytesperline,
            ..Default::default()
        }
    }
}

#[doc(hidden)]
mod ioctl {
    use crate::bindings::v4l2_format;
    nix::ioctl_readwrite!(vidioc_g_fmt, b'V', 4, v4l2_format);
    nix::ioctl_readwrite!(vidioc_s_fmt, b'V', 5, v4l2_format);
    nix::ioctl_readwrite!(vidioc_try_fmt, b'V', 64, v4l2_format);
}

#[derive(Debug, Error)]
pub enum GFmtError {
    #[error("Error while converting from V4L2 format")]
    FromV4L2FormatConversionError(#[from] FormatConversionError),
    #[error("Invalid buffer type requested")]
    InvalidBufferType,
    #[error("Unexpected ioctl error: {0}")]
    IoctlError(Error),
}

/// Safe wrapper around the `VIDIOC_G_FMT` ioctl.
pub fn g_fmt<E: Into<FormatConversionError>, T: Fmt<E>, F: AsRawFd>(
    fd: &F,
    queue: QueueType,
) -> Result<T, GFmtError> {
    let mut fmt = bindings::v4l2_format {
        type_: queue as u32,
        ..unsafe { mem::zeroed() }
    };

    match unsafe { ioctl::vidioc_g_fmt(fd.as_raw_fd(), &mut fmt) } {
        Ok(_) => Ok(fmt.try_into().map_err(|e: E| e.into())?),
        Err(Error::Sys(Errno::EINVAL)) => Err(GFmtError::InvalidBufferType),
        Err(e) => Err(GFmtError::IoctlError(e)),
    }
}

#[derive(Debug, Error)]
pub enum SFmtError {
    #[error("Error while converting from V4L2 format")]
    FromV4L2FormatConversionError(#[from] FormatConversionError),
    #[error("Error while converting to V4L2 format")]
    ToV4L2FormatConversionError(FormatConversionError),
    #[error("Invalid buffer type requested")]
    InvalidBufferType,
    #[error("Device currently busy")]
    DeviceBusy,
    #[error("Unexpected ioctl error: {0}")]
    IoctlError(Error),
}

/// Safe wrapper around the `VIDIOC_S_FMT` ioctl.
pub fn s_fmt<E: Into<FormatConversionError>, T: Fmt<E>, F: AsRawFd>(
    fd: &mut F,
    queue: QueueType,
    fmt: Format,
) -> Result<T, SFmtError> {
    let mut fmt: bindings::v4l2_format = (fmt, queue)
        .try_into()
        .map_err(SFmtError::ToV4L2FormatConversionError)?;

    match unsafe { ioctl::vidioc_s_fmt(fd.as_raw_fd(), &mut fmt) } {
        Ok(_) => Ok(fmt.try_into().map_err(|e: E| e.into())?),
        Err(Error::Sys(Errno::EINVAL)) => Err(SFmtError::InvalidBufferType),
        Err(Error::Sys(Errno::EBUSY)) => Err(SFmtError::DeviceBusy),
        Err(e) => Err(SFmtError::IoctlError(e)),
    }
}

#[derive(Debug, Error)]
pub enum TryFmtError {
    #[error("Error while converting from V4L2 format")]
    FromV4L2FormatConversionError(#[from] FormatConversionError),
    #[error("Error while converting to V4L2 format")]
    ToV4L2FormatConversionError(FormatConversionError),
    #[error("Invalid buffer type requested")]
    InvalidBufferType,
    #[error("Unexpected ioctl error: {0}")]
    IoctlError(Error),
}

/// Safe wrapper around the `VIDIOC_TRY_FMT` ioctl.
pub fn try_fmt<E: Into<FormatConversionError>, T: Fmt<E>, F: AsRawFd>(
    fd: &F,
    queue: QueueType,
    fmt: Format,
) -> Result<T, TryFmtError> {
    let mut fmt: bindings::v4l2_format = (fmt, queue)
        .try_into()
        .map_err(TryFmtError::ToV4L2FormatConversionError)?;

    match unsafe { ioctl::vidioc_try_fmt(fd.as_raw_fd(), &mut fmt) } {
        Ok(_) => Ok(fmt.try_into().map_err(|e: E| e.into())?),
        Err(Error::Sys(Errno::EINVAL)) => Err(TryFmtError::InvalidBufferType),
        Err(e) => Err(TryFmtError::IoctlError(e)),
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use std::convert::TryInto;

    #[test]
    // Convert from Format to multi-planar v4l2_format and back.
    fn mplane_to_v4l2_format() {
        // This is not a real format but let us use unique values per field.
        let mplane = Format {
            width: 632,
            height: 480,
            pixelformat: b"NM12".into(),
            plane_fmt: vec![
                PlaneLayout {
                    sizeimage: 307200,
                    bytesperline: 640,
                },
                PlaneLayout {
                    sizeimage: 153600,
                    bytesperline: 320,
                },
                PlaneLayout {
                    sizeimage: 76800,
                    bytesperline: 160,
                },
            ],
        };
        let v4l2_format = bindings::v4l2_format {
            ..(mplane.clone(), QueueType::VideoCaptureMplane)
                .try_into()
                .unwrap()
        };
        let mplane2: Format = v4l2_format.try_into().unwrap();
        assert_eq!(mplane, mplane2);
    }

    #[test]
    // Convert from Format to single-planar v4l2_format and back.
    fn splane_to_v4l2_format() {
        // This is not a real format but let us use unique values per field.
        let splane = Format {
            width: 632,
            height: 480,
            pixelformat: b"NV12".into(),
            plane_fmt: vec![PlaneLayout {
                sizeimage: 307200,
                bytesperline: 640,
            }],
        };
        // Conversion to/from single-planar format.
        let v4l2_format = bindings::v4l2_format {
            ..(splane.clone(), QueueType::VideoCapture)
                .try_into()
                .unwrap()
        };
        let splane2: Format = v4l2_format.try_into().unwrap();
        assert_eq!(splane, splane2);

        // Trying to use a multi-planar format with the single-planar API should
        // fail.
        let mplane = Format {
            width: 632,
            height: 480,
            pixelformat: b"NM12".into(),
            // This is not a real format but let us use unique values per field.
            plane_fmt: vec![
                PlaneLayout {
                    sizeimage: 307200,
                    bytesperline: 640,
                },
                PlaneLayout {
                    sizeimage: 153600,
                    bytesperline: 320,
                },
                PlaneLayout {
                    sizeimage: 76800,
                    bytesperline: 160,
                },
            ],
        };
        assert_eq!(
            TryInto::<bindings::v4l2_format>::try_into((mplane, QueueType::VideoCapture)).err(),
            Some(FormatConversionError::TooManyPlanes(3))
        );
    }
}

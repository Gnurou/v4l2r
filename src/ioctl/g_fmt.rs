//! Safe wrapper for the `VIDIOC_(G|S|TRY)_FMT` ioctls.
use crate::bindings;
use crate::{Error, Result};
use crate::{Format, PixelFormat, PlanePixFormat, QueueType};
use std::convert::{From, Into, TryFrom, TryInto};
use std::default::Default;
use std::mem;
use std::os::unix::io::AsRawFd;

/// Implementors can receive the result from the `g_fmt`, `s_fmt` and `try_fmt`
/// ioctls.
pub trait Fmt: TryFrom<bindings::v4l2_format, Error = Error> {}

impl TryFrom<(Format, QueueType)> for bindings::v4l2_format {
    type Error = Error;

    fn try_from((format, queue): (Format, QueueType)) -> Result<Self> {
        Ok(bindings::v4l2_format {
            type_: queue as u32,
            fmt: match queue {
                QueueType::VideoCaptureMplane | QueueType::VideoOutputMplane => {
                    bindings::v4l2_format__bindgen_ty_1 {
                        pix_mp: {
                            if format.plane_fmt.len() > bindings::VIDEO_MAX_PLANES as usize {
                                return Err(Error::TooManyPlanes);
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
                            return Err(Error::TooManyPlanes);
                        }

                        let (bytesperline, sizeimage) = if format.plane_fmt.len() > 0 {
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
    type Error = Error;
    fn try_from(fmt: bindings::v4l2_format) -> Result<Self> {
        match fmt.type_ {
            bindings::v4l2_buf_type_V4L2_BUF_TYPE_VIDEO_CAPTURE
            | bindings::v4l2_buf_type_V4L2_BUF_TYPE_VIDEO_OUTPUT => {
                let pix = unsafe { &fmt.fmt.pix };
                Ok(Format {
                    width: pix.width,
                    height: pix.height,
                    pixelformat: PixelFormat::from(pix.pixelformat),
                    plane_fmt: vec![PlanePixFormat {
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
                    return Err(Error::TooManyPlanes);
                }

                let mut plane_fmt = Vec::new();
                for i in 0..pix_mp.num_planes as usize {
                    let plane = &pix_mp.plane_fmt[i];
                    plane_fmt.push(PlanePixFormat {
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
            _ => Err(Error::InvalidBufferType),
        }
    }
}

impl Fmt for Format {}

impl Default for bindings::v4l2_plane_pix_format {
    fn default() -> Self {
        bindings::v4l2_plane_pix_format {
            sizeimage: 0,
            bytesperline: 0,
            reserved: Default::default(),
        }
    }
}

impl From<PlanePixFormat> for bindings::v4l2_plane_pix_format {
    fn from(plane: PlanePixFormat) -> Self {
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

/// Safe wrapper around the `VIDIOC_G_FMT` ioctl.
pub fn g_fmt<T: Fmt, F: AsRawFd>(fd: &F, queue: QueueType) -> Result<T> {
    let mut fmt = bindings::v4l2_format {
        type_: queue as u32,
        ..unsafe { mem::zeroed() }
    };

    unsafe { ioctl::vidioc_g_fmt(fd.as_raw_fd(), &mut fmt) }?;

    fmt.try_into()
}

/// Safe wrapper around the `VIDIOC_S_FMT` ioctl.
pub fn s_fmt<T: Fmt, F: AsRawFd>(fd: &mut F, queue: QueueType, fmt: Format) -> Result<T> {
    let mut fmt: bindings::v4l2_format = (fmt, queue).try_into()?;

    unsafe { ioctl::vidioc_s_fmt(fd.as_raw_fd(), &mut fmt) }?;

    fmt.try_into()
}

/// Safe wrapper around the `VIDIOC_TRY_FMT` ioctl.
pub fn try_fmt<T: Fmt, F: AsRawFd>(fd: &mut F, queue: QueueType, fmt: Format) -> Result<T> {
    let mut fmt: bindings::v4l2_format = (fmt, queue).try_into()?;

    unsafe { ioctl::vidioc_try_fmt(fd.as_raw_fd(), &mut fmt) }?;

    fmt.try_into()
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
                PlanePixFormat {
                    sizeimage: 307200,
                    bytesperline: 640,
                },
                PlanePixFormat {
                    sizeimage: 153600,
                    bytesperline: 320,
                },
                PlanePixFormat {
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
            plane_fmt: vec![PlanePixFormat {
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
                PlanePixFormat {
                    sizeimage: 307200,
                    bytesperline: 640,
                },
                PlanePixFormat {
                    sizeimage: 153600,
                    bytesperline: 320,
                },
                PlanePixFormat {
                    sizeimage: 76800,
                    bytesperline: 160,
                },
            ],
        };
        assert_eq!(
            TryInto::<bindings::v4l2_format>::try_into((mplane, QueueType::VideoCapture)).err(),
            Some(Error::TooManyPlanes)
        );
    }
}

use std::os::unix::io::AsRawFd;

use nix::errno::Errno;
use thiserror::Error;

use crate::bindings::v4l2_jpegcompression;

#[doc(hidden)]
mod ioctl {
    use crate::bindings::v4l2_jpegcompression;

    nix::ioctl_read!(vidioc_g_jpegcomp, b'V', 61, v4l2_jpegcompression);
    nix::ioctl_write_ptr!(vidioc_s_jpegcomp, b'V', 62, v4l2_jpegcompression);
}

#[derive(Debug, Error)]
pub enum GJpegCompError {
    #[error("ioctl error: {0}")]
    IoctlError(Errno),
}

impl From<GJpegCompError> for Errno {
    fn from(err: GJpegCompError) -> Self {
        match err {
            GJpegCompError::IoctlError(e) => e,
        }
    }
}

/// Safe wrapper around the `VIDIOC_G_JPEGCOMP` ioctl.
pub fn g_jpegcomp<O: From<v4l2_jpegcompression>>(fd: &impl AsRawFd) -> Result<O, GJpegCompError> {
    let mut jpegcomp: v4l2_jpegcompression = Default::default();

    match unsafe { ioctl::vidioc_g_jpegcomp(fd.as_raw_fd(), &mut jpegcomp) } {
        Ok(_) => Ok(O::from(jpegcomp)),
        Err(e) => Err(GJpegCompError::IoctlError(e)),
    }
}

/// Safe wrapper around the `VIDIOC_S_JPEGCOMP` ioctl.
pub fn s_jpegcomp<I: Into<v4l2_jpegcompression>>(
    fd: &impl AsRawFd,
    jpegcomp: I,
) -> Result<(), GJpegCompError> {
    let jpegcomp: v4l2_jpegcompression = jpegcomp.into();

    match unsafe { ioctl::vidioc_s_jpegcomp(fd.as_raw_fd(), &jpegcomp) } {
        Ok(_) => Ok(()),
        Err(e) => Err(GJpegCompError::IoctlError(e)),
    }
}

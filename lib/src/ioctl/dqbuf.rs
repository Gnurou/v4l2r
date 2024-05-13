use crate::bindings::v4l2_buffer;
use crate::ioctl::ioctl_and_convert;
use crate::ioctl::IoctlConvertError;
use crate::ioctl::IoctlConvertResult;
use crate::ioctl::UncheckedV4l2Buffer;
use crate::QueueType;

use std::convert::TryFrom;
use std::fmt::Debug;
use std::os::unix::io::AsRawFd;

use nix::errno::Errno;
use thiserror::Error;

#[doc(hidden)]
mod ioctl {
    use crate::bindings::v4l2_buffer;
    nix::ioctl_readwrite!(vidioc_dqbuf, b'V', 17, v4l2_buffer);
}

#[derive(Debug, Error)]
pub enum DqBufIoctlError {
    #[error("end-of-stream reached")]
    Eos,
    #[error("no buffer ready for dequeue")]
    NotReady,
    #[error("ioctl error: {0}")]
    IoctlError(Errno),
}

impl From<Errno> for DqBufIoctlError {
    fn from(error: Errno) -> Self {
        match error {
            Errno::EAGAIN => Self::NotReady,
            Errno::EPIPE => Self::Eos,
            error => Self::IoctlError(error),
        }
    }
}

impl From<DqBufIoctlError> for Errno {
    fn from(err: DqBufIoctlError) -> Self {
        match err {
            DqBufIoctlError::Eos => Errno::EPIPE,
            DqBufIoctlError::NotReady => Errno::EAGAIN,
            DqBufIoctlError::IoctlError(e) => e,
        }
    }
}

pub type DqBufError<CE> = IoctlConvertError<DqBufIoctlError, CE>;
pub type DqBufResult<O, CE> = IoctlConvertResult<O, DqBufIoctlError, CE>;

/// Safe wrapper around the `VIDIOC_DQBUF` ioctl.
pub fn dqbuf<O>(fd: &impl AsRawFd, queue: QueueType) -> DqBufResult<O, O::Error>
where
    O: TryFrom<UncheckedV4l2Buffer>,
    O::Error: std::fmt::Debug,
{
    let mut v4l2_buf = UncheckedV4l2Buffer(
        v4l2_buffer {
            type_: queue as u32,
            ..Default::default()
        },
        Default::default(),
    );

    if queue.is_multiplanar() {
        let planes = v4l2_buf.1.get_or_insert(Default::default());
        v4l2_buf.0.m.planes = planes.as_mut_ptr();
        v4l2_buf.0.length = planes.len() as u32;
    }

    ioctl_and_convert(
        unsafe { ioctl::vidioc_dqbuf(fd.as_raw_fd(), &mut v4l2_buf.0) }
            .map(|_| v4l2_buf)
            .map_err(Into::into),
    )
}

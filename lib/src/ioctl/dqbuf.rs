use crate::bindings;
use crate::ioctl::is_multi_planar;
use crate::ioctl::QueryBuf;
use crate::ioctl::V4l2BufferPlanes;
use crate::QueueType;

use std::fmt::Debug;
use std::mem;
use std::os::unix::io::AsRawFd;

use nix::errno::Errno;
use thiserror::Error;

#[doc(hidden)]
mod ioctl {
    use crate::bindings::v4l2_buffer;
    nix::ioctl_readwrite!(vidioc_dqbuf, b'V', 17, v4l2_buffer);
}

#[derive(Debug, Error)]
pub enum DqBufError {
    #[error("end-of-stream reached")]
    Eos,
    #[error("no buffer ready for dequeue")]
    NotReady,
    #[error("ioctl error: {0}")]
    IoctlError(Errno),
}

impl From<Errno> for DqBufError {
    fn from(error: Errno) -> Self {
        match error {
            Errno::EAGAIN => Self::NotReady,
            Errno::EPIPE => Self::Eos,
            error => Self::IoctlError(error),
        }
    }
}

impl From<DqBufError> for Errno {
    fn from(err: DqBufError) -> Self {
        match err {
            DqBufError::Eos => Errno::EPIPE,
            DqBufError::NotReady => Errno::EAGAIN,
            DqBufError::IoctlError(e) => e,
        }
    }
}

pub type DqBufResult<T> = Result<T, DqBufError>;

/// Safe wrapper around the `VIDIOC_DQBUF` ioctl.
pub fn dqbuf<T: QueryBuf>(fd: &impl AsRawFd, queue: QueueType) -> DqBufResult<T> {
    let mut v4l2_buf = bindings::v4l2_buffer {
        type_: queue as u32,
        ..unsafe { mem::zeroed() }
    };

    let dequeued_buffer = if is_multi_planar(queue) {
        let mut plane_data: V4l2BufferPlanes = Default::default();
        v4l2_buf.m.planes = plane_data.as_mut_ptr();
        v4l2_buf.length = plane_data.len() as u32;

        unsafe { ioctl::vidioc_dqbuf(fd.as_raw_fd(), &mut v4l2_buf) }?;
        T::from_v4l2_buffer(v4l2_buf, Some(plane_data))
    } else {
        unsafe { ioctl::vidioc_dqbuf(fd.as_raw_fd(), &mut v4l2_buf) }?;
        T::from_v4l2_buffer(v4l2_buf, None)
    };

    Ok(dequeued_buffer)
}

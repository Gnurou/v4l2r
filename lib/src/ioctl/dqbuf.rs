use super::{is_multi_planar, BufferFlags, PlaneData};
use crate::bindings;
use crate::QueueType;

use nix::{self, errno::Errno, Error};
use std::mem;
use std::os::unix::io::AsRawFd;
use std::{fmt::Debug, pin::Pin};
use thiserror::Error;

/// Implementors can receive the result from the `dqbuf` ioctl.
pub trait DqBuf: Sized {
    /// Try to retrieve the data from `v4l2_buf`. If `v4l2_planes` is `None`,
    /// then the buffer is single-planar. If it has data, the buffer is
    /// multi-planar and `v4l2_planes` shall be used to retrieve the plane data.
    fn from_v4l2_buffer(v4l2_buf: bindings::v4l2_buffer, v4l2_planes: Option<PlaneData>) -> Self;
}

impl DqBuf for bindings::v4l2_buffer {
    fn from_v4l2_buffer(v4l2_buf: bindings::v4l2_buffer, _v4l2_planes: Option<PlaneData>) -> Self {
        v4l2_buf
    }
}

/// Allows to dequeue a buffer without caring for any of its data.
impl DqBuf for () {
    fn from_v4l2_buffer(_v4l2_buf: bindings::v4l2_buffer, _v4l2_planes: Option<PlaneData>) -> Self {
    }
}

/// Useful for the case where we are only interested in the index of a dequeued
/// buffer
impl DqBuf for u32 {
    fn from_v4l2_buffer(v4l2_buf: bindings::v4l2_buffer, _v4l2_planes: Option<PlaneData>) -> Self {
        v4l2_buf.index
    }
}

/// Information about a single plane of a dequeued buffer.
pub struct DqBufPlane<'a> {
    plane: &'a bindings::v4l2_plane,
}

impl<'a> Debug for DqBufPlane<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DQBufPlane")
            .field("length", &self.length())
            .field("bytesused", &self.bytesused())
            .field("data_offset", &self.data_offset())
            .finish()
    }
}

impl<'a> DqBufPlane<'a> {
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

/// Information for a dequeued buffer. Safe variant of `struct v4l2_buffer`.
pub struct DqBuffer {
    v4l2_buffer: bindings::v4l2_buffer,
    // The `m.planes` pointer of `v4l2_buffer` points here and must stay stable
    // as we move this object around.
    v4l2_planes: Pin<Box<PlaneData>>,
}

impl Clone for DqBuffer {
    fn clone(&self) -> Self {
        let mut ret = Self {
            v4l2_buffer: self.v4l2_buffer,
            v4l2_planes: self.v4l2_planes.clone(),
        };
        // Make the planes pointer of the cloned data point to the right copy.
        if self.is_multi_planar() {
            ret.v4l2_buffer.m.planes = ret.v4l2_planes.as_mut_ptr();
        }
        ret
    }
}

/// DQBuffer is safe to send across threads. `v4l2_buffer` is !Send because it
/// contains a pointer, but we are making sure to use it safely here.
unsafe impl Send for DqBuffer {}

impl Debug for DqBuffer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DQBuffer")
            .field("index", &self.index())
            .field("flags", &self.flags())
            .field("sequence", &self.sequence())
            .finish()
    }
}

impl DqBuffer {
    pub fn index(&self) -> u32 {
        self.v4l2_buffer.index
    }

    pub fn flags(&self) -> BufferFlags {
        BufferFlags::from_bits_truncate(self.v4l2_buffer.flags)
    }

    pub fn is_last(&self) -> bool {
        self.flags().contains(BufferFlags::LAST)
    }

    pub fn timestamp(&self) -> bindings::timeval {
        self.v4l2_buffer.timestamp
    }

    pub fn sequence(&self) -> u32 {
        self.v4l2_buffer.sequence
    }

    pub fn is_multi_planar(&self) -> bool {
        matches!(
            self.v4l2_buffer.type_,
            bindings::v4l2_buf_type_V4L2_BUF_TYPE_VIDEO_OUTPUT_MPLANE
                | bindings::v4l2_buf_type_V4L2_BUF_TYPE_VIDEO_CAPTURE_MPLANE
        )
    }

    pub fn num_planes(&self) -> usize {
        if self.is_multi_planar() {
            self.v4l2_buffer.length as usize
        } else {
            1
        }
    }

    /// Returns the first plane of the buffer. This method is guaranteed to
    /// succeed because every buffer has at least one plane.
    pub fn get_first_plane(&self) -> DqBufPlane {
        DqBufPlane {
            plane: &self.v4l2_planes[0],
        }
    }

    /// Returns plane `index` of the buffer, or `None` if `index` is larger than
    /// the number of planes in this buffer.
    pub fn get_plane(&self, index: usize) -> Option<DqBufPlane> {
        if index < self.num_planes() {
            Some(DqBufPlane {
                plane: &self.v4l2_planes[index],
            })
        } else {
            None
        }
    }

    /// Returns the raw v4l2_buffer as a pointer. Useful to pass to unsafe
    /// non-Rust code.
    pub fn as_raw_v4l2_buffer(&self) -> *const bindings::v4l2_buffer {
        &self.v4l2_buffer
    }
}

impl DqBuf for DqBuffer {
    fn from_v4l2_buffer(
        v4l2_buffer: bindings::v4l2_buffer,
        v4l2_planes: Option<PlaneData>,
    ) -> Self {
        let mut dqbuf = DqBuffer {
            v4l2_buffer,
            v4l2_planes: Box::pin(match v4l2_planes {
                Some(planes) => planes,
                // In single-plane mode, reproduce the buffer information into
                // a v4l2_plane in order to present a unified interface.
                None => {
                    let mut pdata: PlaneData = Default::default();
                    pdata[0] = bindings::v4l2_plane {
                        bytesused: v4l2_buffer.bytesused,
                        length: v4l2_buffer.length,
                        data_offset: 0,
                        reserved: Default::default(),
                        // Safe because both unions have the same members and
                        // layout in single-plane mode.
                        m: unsafe { std::mem::transmute(v4l2_buffer.m) },
                    };

                    pdata
                }
            }),
        };

        // Since the planes have moved, update the planes pointer if we are
        // using multi-planar.
        if dqbuf.is_multi_planar() {
            dqbuf.v4l2_buffer.m.planes = dqbuf.v4l2_planes.as_mut_ptr()
        }

        dqbuf
    }
}

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
    IoctlError(Error),
}

impl From<Error> for DqBufError {
    fn from(error: Error) -> Self {
        match error {
            Errno::EAGAIN => Self::NotReady,
            Errno::EPIPE => Self::Eos,
            error => Self::IoctlError(error),
        }
    }
}

pub type DqBufResult<T> = Result<T, DqBufError>;

/// Safe wrapper around the `VIDIOC_DQBUF` ioctl.
pub fn dqbuf<T: DqBuf + Debug, F: AsRawFd>(fd: &F, queue: QueueType) -> DqBufResult<T> {
    let mut v4l2_buf = bindings::v4l2_buffer {
        type_: queue as u32,
        ..unsafe { mem::zeroed() }
    };

    let dequeued_buffer = if is_multi_planar(queue) {
        let mut plane_data: PlaneData = Default::default();
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

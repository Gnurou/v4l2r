use super::{is_multi_planar, BufferFlags, V4l2BufferPlanes};
use crate::bindings;
use crate::ioctl::V4l2Buffer;
use crate::QueueType;
use nix::errno::Errno;
use thiserror::Error;

use std::mem;
use std::os::unix::io::AsRawFd;

/// Implementors can receive the result from the `querybuf` ioctl.
pub trait QueryBuf: Sized {
    /// Try to retrieve the data from `v4l2_buf`. If `v4l2_planes` is `None`,
    /// then the buffer is single-planar. If it has data, the buffer is
    /// multi-planar and the array of `struct v4l2_plane` shall be used to
    /// retrieve the plane data.
    fn from_v4l2_buffer(
        v4l2_buf: bindings::v4l2_buffer,
        v4l2_planes: Option<V4l2BufferPlanes>,
    ) -> Self;
}

/// For cases where we are not interested in the result of `qbuf`
impl QueryBuf for () {
    fn from_v4l2_buffer(
        _v4l2_buf: bindings::v4l2_buffer,
        _v4l2_planes: Option<V4l2BufferPlanes>,
    ) -> Self {
    }
}

impl QueryBuf for V4l2Buffer {
    fn from_v4l2_buffer(
        v4l2_buf: bindings::v4l2_buffer,
        v4l2_planes: Option<V4l2BufferPlanes>,
    ) -> Self {
        Self {
            buffer: v4l2_buf,
            planes: v4l2_planes.unwrap_or_else(|| {
                let mut pdata: V4l2BufferPlanes = Default::default();
                // Duplicate information for the first plane so our methods can work.
                pdata[0] = bindings::v4l2_plane {
                    bytesused: v4l2_buf.bytesused,
                    length: v4l2_buf.length,
                    data_offset: 0,
                    reserved: Default::default(),
                    // Safe because both unions have the same members and
                    // layout in single-plane mode.
                    m: unsafe { std::mem::transmute(v4l2_buf.m) },
                };

                pdata
            }),
        }
    }
}

#[derive(Debug)]
pub struct QueryBufPlane {
    /// Offset to pass to `mmap()` in order to obtain a mapping for this plane.
    pub mem_offset: u32,
    /// Length of this plane.
    pub length: u32,
}

/// Contains all the information that makes sense when using `querybuf`.
#[derive(Debug)]
pub struct QueryBuffer {
    pub index: usize,
    pub flags: BufferFlags,
    pub planes: Vec<QueryBufPlane>,
}

impl QueryBuf for QueryBuffer {
    fn from_v4l2_buffer(
        v4l2_buf: bindings::v4l2_buffer,
        v4l2_planes: Option<V4l2BufferPlanes>,
    ) -> Self {
        let planes = match v4l2_planes {
            None => vec![QueryBufPlane {
                mem_offset: unsafe { v4l2_buf.m.offset },
                length: v4l2_buf.length,
            }],
            Some(v4l2_planes) => v4l2_planes
                .iter()
                .take(v4l2_buf.length as usize)
                .map(|v4l2_plane| QueryBufPlane {
                    mem_offset: unsafe { v4l2_plane.m.mem_offset },
                    length: v4l2_plane.length,
                })
                .collect(),
        };

        QueryBuffer {
            index: v4l2_buf.index as usize,
            flags: BufferFlags::from_bits_truncate(v4l2_buf.flags),
            planes,
        }
    }
}

#[doc(hidden)]
mod ioctl {
    use crate::bindings::v4l2_buffer;
    nix::ioctl_readwrite!(vidioc_querybuf, b'V', 9, v4l2_buffer);
}

#[derive(Debug, Error)]
pub enum QueryBufError {
    #[error("ioctl error: {0}")]
    IoctlError(#[from] Errno),
}

impl From<QueryBufError> for Errno {
    fn from(err: QueryBufError) -> Self {
        match err {
            QueryBufError::IoctlError(e) => e,
        }
    }
}

/// Safe wrapper around the `VIDIOC_QUERYBUF` ioctl.
pub fn querybuf<T: QueryBuf>(
    fd: &impl AsRawFd,
    queue: QueueType,
    index: usize,
) -> Result<T, QueryBufError> {
    let mut v4l2_buf = bindings::v4l2_buffer {
        index: index as u32,
        type_: queue as u32,
        ..unsafe { mem::zeroed() }
    };

    if is_multi_planar(queue) {
        let mut plane_data: V4l2BufferPlanes = Default::default();
        v4l2_buf.m.planes = plane_data.as_mut_ptr();
        v4l2_buf.length = plane_data.len() as u32;

        unsafe { ioctl::vidioc_querybuf(fd.as_raw_fd(), &mut v4l2_buf) }?;
        Ok(T::from_v4l2_buffer(v4l2_buf, Some(plane_data)))
    } else {
        unsafe { ioctl::vidioc_querybuf(fd.as_raw_fd(), &mut v4l2_buf) }?;
        Ok(T::from_v4l2_buffer(v4l2_buf, None))
    }
}

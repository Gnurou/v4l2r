use super::{is_multi_planar, BufferFlags, PlaneData};
use crate::bindings;
use crate::memory::MemoryType;
use crate::QueueType;
use crate::{Error, Result};

use std::mem;
use std::os::unix::io::AsRawFd;

/// Implementors can receive the result from the `querybuf` ioctl.
pub trait QueryBuf: Sized {
    /// Try to retrieve the data from `v4l2_buf`. If `v4l2_planes` is `None`,
    /// then the buffer is single-planar. If it has data, the buffer is
    /// multi-planar and the array of `struct v4l2_plane` shall be used to
    /// retrieve the plane data.
    fn from_v4l2_buffer(
        v4l2_buf: &bindings::v4l2_buffer,
        v4l2_planes: Option<&PlaneData>,
    ) -> Result<Self>;
}

#[derive(Debug)]
pub struct QueryBufPlane {
    pub length: u32,
}

/// Contains all the information that makes sense when using `querybuf`.
#[derive(Debug)]
pub struct QueryBuffer {
    pub flags: BufferFlags,
    pub planes: Vec<QueryBufPlane>,
}

impl QueryBuf for QueryBuffer {
    fn from_v4l2_buffer(
        v4l2_buf: &bindings::v4l2_buffer,
        v4l2_planes: Option<&PlaneData>,
    ) -> Result<Self> {
        let planes = match v4l2_planes {
            None => vec![QueryBufPlane {
                length: v4l2_buf.length,
            }],
            Some(v4l2_planes) => v4l2_planes
                .iter()
                .take(v4l2_buf.length as usize)
                .map(|v4l2_plane| QueryBufPlane {
                    length: v4l2_plane.length,
                })
                .collect(),
        };

        Ok(QueryBuffer {
            flags: BufferFlags::from_bits_truncate(v4l2_buf.flags),
            planes,
        })
    }
}

#[derive(Debug)]
pub struct QueryBufPlaneMMAP {
    pub length: u32,
    pub mem_offset: u32,
}

/// Contains all the information that makes sense when using `querybuf`.
#[derive(Debug)]
pub struct QueryBufferMMAP {
    pub flags: BufferFlags,
    pub planes: Vec<QueryBufPlaneMMAP>,
}

impl QueryBuf for QueryBufferMMAP {
    fn from_v4l2_buffer(
        v4l2_buf: &bindings::v4l2_buffer,
        v4l2_planes: Option<&PlaneData>,
    ) -> Result<Self> {
        if v4l2_buf.memory != MemoryType::MMAP as u32 {
            return Err(Error::WrongMemoryType);
        }

        let planes = match v4l2_planes {
            None => vec![QueryBufPlaneMMAP {
                length: v4l2_buf.length,
                mem_offset: unsafe { v4l2_buf.m.offset },
            }],
            Some(v4l2_planes) => v4l2_planes
                .iter()
                .take(v4l2_buf.length as usize)
                .map(|v4l2_plane| QueryBufPlaneMMAP {
                    length: v4l2_plane.length,
                    mem_offset: unsafe { v4l2_plane.m.mem_offset },
                })
                .collect(),
        };

        Ok(QueryBufferMMAP {
            flags: BufferFlags::from_bits_truncate(v4l2_buf.flags),
            planes,
        })
    }
}

#[doc(hidden)]
mod ioctl {
    use crate::bindings::v4l2_buffer;
    nix::ioctl_readwrite!(vidioc_querybuf, b'V', 9, v4l2_buffer);
}

/// Safe wrapper around the `VIDIOC_QUERYBUF` ioctl.
pub fn querybuf<T: QueryBuf, F: AsRawFd>(fd: &F, queue: QueueType, index: usize) -> Result<T> {
    let mut v4l2_buf = bindings::v4l2_buffer {
        index: index as u32,
        type_: queue as u32,
        ..unsafe { mem::zeroed() }
    };

    if is_multi_planar(queue) {
        let mut plane_data: PlaneData = Default::default();
        v4l2_buf.m.planes = plane_data.as_mut_ptr();
        v4l2_buf.length = plane_data.len() as u32;

        unsafe { ioctl::vidioc_querybuf(fd.as_raw_fd(), &mut v4l2_buf) }?;
        Ok(T::from_v4l2_buffer(&v4l2_buf, Some(&plane_data))?)
    } else {
        unsafe { ioctl::vidioc_querybuf(fd.as_raw_fd(), &mut v4l2_buf) }?;
        Ok(T::from_v4l2_buffer(&v4l2_buf, None)?)
    }
}

use std::convert::Infallible;
use std::convert::TryFrom;
use std::os::unix::io::AsRawFd;

use nix::errno::Errno;
use thiserror::Error;

use crate::bindings::v4l2_buffer;
use crate::ioctl::BufferFlags;
use crate::ioctl::UncheckedV4l2Buffer;
use crate::ioctl::V4l2BufferPlanes;
use crate::QueueType;

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

impl TryFrom<UncheckedV4l2Buffer> for QueryBuffer {
    type Error = Infallible;

    fn try_from(buffer: UncheckedV4l2Buffer) -> Result<Self, Self::Error> {
        let v4l2_buf = buffer.0;
        let planes = match buffer.1 {
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

        Ok(QueryBuffer {
            index: v4l2_buf.index as usize,
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

#[derive(Debug, Error)]
pub enum QueryBufError<Q: TryFrom<UncheckedV4l2Buffer>> {
    #[error("error while converting from v4l2_buffer")]
    ConversionError(Q::Error),
    #[error("ioctl error: {0}")]
    IoctlError(#[from] Errno),
}

impl<Q: TryFrom<UncheckedV4l2Buffer>> From<QueryBufError<Q>> for Errno {
    fn from(err: QueryBufError<Q>) -> Self {
        match err {
            QueryBufError::ConversionError(_) => Errno::EINVAL,
            QueryBufError::IoctlError(e) => e,
        }
    }
}

/// Safe wrapper around the `VIDIOC_QUERYBUF` ioctl.
pub fn querybuf<T: TryFrom<UncheckedV4l2Buffer>>(
    fd: &impl AsRawFd,
    queue: QueueType,
    index: usize,
) -> Result<T, QueryBufError<T>> {
    let mut v4l2_buf = v4l2_buffer {
        index: index as u32,
        type_: queue as u32,
        ..Default::default()
    };

    if queue.is_multiplanar() {
        let mut plane_data: V4l2BufferPlanes = Default::default();
        v4l2_buf.m.planes = plane_data.as_mut_ptr();
        v4l2_buf.length = plane_data.len() as u32;

        unsafe { ioctl::vidioc_querybuf(fd.as_raw_fd(), &mut v4l2_buf) }?;
        Ok(T::try_from(UncheckedV4l2Buffer(v4l2_buf, Some(plane_data)))
            .map_err(QueryBufError::ConversionError)?)
    } else {
        unsafe { ioctl::vidioc_querybuf(fd.as_raw_fd(), &mut v4l2_buf) }?;
        Ok(T::try_from(UncheckedV4l2Buffer(v4l2_buf, None))
            .map_err(QueryBufError::ConversionError)?)
    }
}

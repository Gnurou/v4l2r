//! Safe wrapper for the VIDIOC_(D)QBUF and VIDIOC_QUERYBUF ioctls.
use crate::{bindings, Error, PlaneHandle, QueueType, Result};
use bitflags::bitflags;
use std::cmp::Ordering;
use std::fmt::Debug;
use std::mem;
use std::os::unix::io::AsRawFd;

bitflags! {
    /// Flags corresponding to the `flags` field of `struct v4l2_buffer`.
    /// TODO split into two types, one for the user -> kernel and another for
    /// the kernel -> user direction to filter invalid flags?
    #[derive(Default)]
    pub struct BufferFlags: u32 {
        const MAPPED = bindings::V4L2_BUF_FLAG_MAPPED;
        const QUEUED = bindings::V4L2_BUF_FLAG_QUEUED;
        const DONE = bindings::V4L2_BUF_FLAG_DONE;
        const ERROR = bindings::V4L2_BUF_FLAG_ERROR;

        const LAST = bindings::V4L2_BUF_FLAG_LAST;
    }
}

/// Implementors can pass buffer data to the `qbuf` ioctl.
pub trait QBuf {
    /// Fill the buffer information into the single-planar `v4l2_buf`. Fail if
    /// the number of planes is different from 1.
    fn fill_splane_v4l2_buffer(self, v4l2_buf: &mut bindings::v4l2_buffer) -> Result<()>;
    /// Fill the buffer information into the multi-planar `v4l2_buf`, using
    /// `v4l2_planes` to store the plane data. Fail if the number of planes is
    /// not between 1 and `VIDEO_MAX_PLANES` included.
    fn fill_mplane_v4l2_buffer(
        self,
        v4l2_buf: &mut bindings::v4l2_buffer,
        v4l2_planes: &mut [bindings::v4l2_plane; bindings::VIDEO_MAX_PLANES as usize],
    ) -> Result<()>;
}

/// Representation of a single plane of a V4L2 buffer.
#[derive(Debug, Default)]
pub struct QBufPlane<H: PlaneHandle> {
    pub bytesused: u32,
    // This is only valid for MPlane queues. SPlanes don't have an equivalent.
    pub data_offset: u32,
    pub handle: H,
}

/// Contains all the information that can be passed to the `qbuf` ioctl.
#[derive(Debug, Default)]
pub struct QBuffer<H: PlaneHandle> {
    pub flags: BufferFlags,
    pub field: u32,
    pub sequence: u32,
    pub planes: Vec<QBufPlane<H>>,
}

impl<H: PlaneHandle> QBuf for QBuffer<H> {
    fn fill_splane_v4l2_buffer(self, v4l2_buf: &mut bindings::v4l2_buffer) -> Result<()> {
        match self.planes.len().cmp(&1) {
            Ordering::Less => return Err(Error::NotEnoughPlanes),
            Ordering::Greater => return Err(Error::TooManyPlanes),
            Ordering::Equal => (),
        };

        let plane = &self.planes[0];
        if plane.data_offset != 0 {
            return Err(Error::DataOffsetNotSupported);
        }
        v4l2_buf.memory = H::MEMORY_TYPE as u32;
        v4l2_buf.bytesused = plane.bytesused;
        plane.handle.fill_v4l2_buffer(v4l2_buf);

        Ok(())
    }

    fn fill_mplane_v4l2_buffer(
        self,
        v4l2_buf: &mut bindings::v4l2_buffer,
        v4l2_planes: &mut [bindings::v4l2_plane; bindings::VIDEO_MAX_PLANES as usize],
    ) -> Result<()> {
        if self.planes.len() == 0 {
            return Err(Error::NotEnoughPlanes);
        }
        if self.planes.len() > v4l2_planes.len() {
            return Err(Error::TooManyPlanes);
        }

        v4l2_buf.memory = H::MEMORY_TYPE as u32;
        v4l2_buf.length = self.planes.len() as u32;
        v4l2_planes
            .iter_mut()
            .zip(self.planes.into_iter())
            .for_each(|(v4l2_plane, plane)| {
                v4l2_plane.bytesused = plane.bytesused;
                v4l2_plane.data_offset = plane.data_offset;
                plane.handle.fill_v4l2_plane(v4l2_plane);
            });

        Ok(())
    }
}

/// Implementors can receive the result from the `querybuf` or `dqbuf` ioctls.
pub trait DQBuf: Sized {
    /// Try to retrieve the data from `v4l2_buf`. If `v4l2_planes` is `None`,
    /// then the buffer is single-planar. If it has data, the buffer is
    /// multi-planar and the array of `struct v4l2_plane` shall be used to
    /// retrieve the plane data.
    fn from_v4l2_buffer(
        v4l2_buf: &bindings::v4l2_buffer,
        v4l2_planes: Option<&[bindings::v4l2_plane; bindings::VIDEO_MAX_PLANES as usize]>,
    ) -> Result<Self>;
}

/// Simply dequeue a buffer without caring for any of its data.
impl DQBuf for () {
    fn from_v4l2_buffer(
        _v4l2_buf: &bindings::v4l2_buffer,
        _v4l2_planes: Option<&[bindings::v4l2_plane; bindings::VIDEO_MAX_PLANES as usize]>,
    ) -> Result<Self> {
        Ok(())
    }
}

/// Useful for the case where we are only interested in index of a dequeued
/// buffer
impl DQBuf for u32 {
    fn from_v4l2_buffer(
        v4l2_buf: &bindings::v4l2_buffer,
        _v4l2_planes: Option<&[bindings::v4l2_plane; bindings::VIDEO_MAX_PLANES as usize]>,
    ) -> Result<Self> {
        Ok(v4l2_buf.index)
    }
}

#[derive(Debug)]
pub struct QueryBufPlane<H: PlaneHandle> {
    pub length: u32,
    pub handle: H,
}
/// Contains all the information that makes sense when using `querybuf`.
#[derive(Debug)]
pub struct QueryBuffer<H: PlaneHandle> {
    pub flags: BufferFlags,
    pub planes: Vec<QueryBufPlane<H>>,
}

impl<H: PlaneHandle> DQBuf for QueryBuffer<H> {
    fn from_v4l2_buffer(
        v4l2_buf: &bindings::v4l2_buffer,
        v4l2_planes: Option<&[bindings::v4l2_plane; bindings::VIDEO_MAX_PLANES as usize]>,
    ) -> Result<Self> {
        let planes = match v4l2_planes {
            None => vec![QueryBufPlane {
                length: v4l2_buf.length,
                handle: H::try_from_splane_v4l2_buffer(v4l2_buf)?,
            }],
            Some(v4l2_planes) => {
                let handles = H::try_from_mplane_v4l2_buffer(v4l2_buf, v4l2_planes)?;

                v4l2_planes
                    .iter()
                    .zip(handles)
                    .map(|(v4l2_plane, handle)| QueryBufPlane {
                        length: v4l2_plane.length,
                        handle,
                    })
                    .collect()
            }
        };

        Ok(QueryBuffer {
            flags: BufferFlags::from_bits_truncate(v4l2_buf.flags),
            planes,
        })
    }
}

#[derive(Debug)]
pub struct DQBufPlane<H: PlaneHandle> {
    pub length: u32,
    pub bytesused: u32,
    pub data_offset: u32,
    pub handle: H,
}

/// Contains all the information from a dequeued buffer. Safe variant of
/// `struct v4l2_buffer`.
#[derive(Debug, Default)]
pub struct DQBuffer<H: PlaneHandle> {
    pub index: u32,
    pub flags: BufferFlags,
    pub field: u32,
    pub sequence: u32,
    pub planes: Vec<DQBufPlane<H>>,
}

impl Default for bindings::v4l2_plane {
    fn default() -> Self {
        unsafe { mem::zeroed() }
    }
}

impl<H: PlaneHandle> DQBuf for DQBuffer<H> {
    fn from_v4l2_buffer(
        v4l2_buf: &bindings::v4l2_buffer,
        v4l2_planes: Option<&[bindings::v4l2_plane; bindings::VIDEO_MAX_PLANES as usize]>,
    ) -> Result<Self> {
        let planes = match v4l2_planes {
            None => vec![DQBufPlane {
                length: v4l2_buf.length,
                bytesused: v4l2_buf.bytesused,
                data_offset: 0,
                handle: H::try_from_splane_v4l2_buffer(v4l2_buf)?,
            }],
            Some(v4l2_planes) => {
                let handles = H::try_from_mplane_v4l2_buffer(v4l2_buf, v4l2_planes)?;

                v4l2_planes
                    .iter()
                    .zip(handles)
                    .map(|(v4l2_plane, handle)| DQBufPlane {
                        length: v4l2_plane.length,
                        bytesused: v4l2_plane.bytesused,
                        data_offset: v4l2_plane.data_offset,
                        handle,
                    })
                    .collect()
            }
        };

        Ok(DQBuffer {
            index: v4l2_buf.index as u32,
            flags: BufferFlags::from_bits_truncate(v4l2_buf.flags),
            field: v4l2_buf.field,
            sequence: v4l2_buf.sequence,
            planes,
        })
    }
}

#[doc(hidden)]
mod ioctl {
    use crate::bindings::v4l2_buffer;
    nix::ioctl_readwrite!(vidioc_querybuf, b'V', 9, v4l2_buffer);
    nix::ioctl_readwrite!(vidioc_qbuf, b'V', 15, v4l2_buffer);
    nix::ioctl_readwrite!(vidioc_dqbuf, b'V', 17, v4l2_buffer);
}

fn is_multi_planar(queue: QueueType) -> bool {
    match queue {
        QueueType::VideoCaptureMplane | QueueType::VideoOutputMplane => true,
        _ => false,
    }
}

fn qbuf_mp<T: QBuf, F: AsRawFd>(
    fd: &F,
    v4l2_buf: &mut bindings::v4l2_buffer,
    buf_data: T,
) -> Result<()> {
    let mut plane_data: [bindings::v4l2_plane; bindings::VIDEO_MAX_PLANES as usize] =
        Default::default();
    v4l2_buf.m.planes = plane_data.as_mut_ptr();
    buf_data.fill_mplane_v4l2_buffer(v4l2_buf, &mut plane_data)?;

    unsafe { ioctl::vidioc_qbuf(fd.as_raw_fd(), v4l2_buf) }?;

    Ok(())
}

/// Safe wrapper around the `VIDIOC_QBUF` ioctl.
pub fn qbuf<T: QBuf, F: AsRawFd>(
    fd: &F,
    queue: QueueType,
    index: usize,
    buf_data: T,
) -> Result<()> {
    let mut v4l2_buf = bindings::v4l2_buffer {
        index: index as u32,
        type_: queue as u32,
        ..unsafe { mem::zeroed() }
    };

    if is_multi_planar(queue) {
        return qbuf_mp(fd, &mut v4l2_buf, buf_data);
    }

    buf_data.fill_splane_v4l2_buffer(&mut v4l2_buf)?;
    unsafe { ioctl::vidioc_qbuf(fd.as_raw_fd(), &mut v4l2_buf) }?;
    Ok(())
}

fn dqbuf_mp<T: DQBuf, F: AsRawFd>(fd: &F, v4l2_buf: &mut bindings::v4l2_buffer) -> Result<T> {
    let mut plane_data: [bindings::v4l2_plane; bindings::VIDEO_MAX_PLANES as usize] =
        Default::default();
    v4l2_buf.m.planes = plane_data.as_mut_ptr();
    v4l2_buf.length = plane_data.len() as u32;

    unsafe { ioctl::vidioc_dqbuf(fd.as_raw_fd(), v4l2_buf) }?;

    Ok(T::from_v4l2_buffer(v4l2_buf, Some(&plane_data))?)
}

/// Safe wrapper around the `VIDIOC_DQBUF` ioctl.
pub fn dqbuf<T: DQBuf, F: AsRawFd>(fd: &F, queue: QueueType) -> Result<T> {
    let mut v4l2_buf = bindings::v4l2_buffer {
        type_: queue as u32,
        ..unsafe { mem::zeroed() }
    };
    if is_multi_planar(queue) {
        return dqbuf_mp(fd, &mut v4l2_buf);
    }

    unsafe { ioctl::vidioc_dqbuf(fd.as_raw_fd(), &mut v4l2_buf) }?;

    Ok(T::from_v4l2_buffer(&v4l2_buf, None)?)
}

fn querybuf_mp<T: DQBuf, F: AsRawFd>(fd: &F, v4l2_buf: &mut bindings::v4l2_buffer) -> Result<T> {
    let mut plane_data: [bindings::v4l2_plane; bindings::VIDEO_MAX_PLANES as usize] =
        Default::default();
    v4l2_buf.m.planes = plane_data.as_mut_ptr();
    v4l2_buf.length = plane_data.len() as u32;

    unsafe { ioctl::vidioc_querybuf(fd.as_raw_fd(), v4l2_buf) }?;

    Ok(T::from_v4l2_buffer(v4l2_buf, Some(&plane_data))?)
}

/// Safe wrapper around the `VIDIOC_QUERYBUF` ioctl.
pub fn querybuf<T: DQBuf, F: AsRawFd>(fd: &F, queue: QueueType, index: usize) -> Result<T> {
    let mut v4l2_buf = bindings::v4l2_buffer {
        index: index as u32,
        type_: queue as u32,
        ..unsafe { mem::zeroed() }
    };
    if is_multi_planar(queue) {
        return querybuf_mp(fd, &mut v4l2_buf);
    }

    unsafe { ioctl::vidioc_querybuf(fd.as_raw_fd(), &mut v4l2_buf) }?;

    Ok(T::from_v4l2_buffer(&v4l2_buf, None)?)
}

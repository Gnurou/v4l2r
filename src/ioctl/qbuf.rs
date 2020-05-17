//! Safe wrapper for the VIDIOC_(D)QBUF and VIDIOC_QUERYBUF ioctls.
use super::{is_multi_planar, PlaneData};
use crate::memory::PlaneHandle;
use crate::{bindings, Error, QueueType, Result};

use bitflags::bitflags;
use std::cmp::Ordering;
use std::fmt::Debug;
use std::mem;
use std::os::unix::io::AsRawFd;

/// For simple initialization of `PlaneData`.
impl Default for bindings::v4l2_plane {
    fn default() -> Self {
        unsafe { mem::zeroed() }
    }
}

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
        v4l2_planes: &mut PlaneData,
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

impl<H: PlaneHandle> QBufPlane<H> {
    pub fn new(handle: H, bytes_used: usize) -> Self {
        QBufPlane {
            bytesused: bytes_used as u32,
            data_offset: 0,
            handle,
        }
    }
}

/// Contains all the information that can be passed to the `qbuf` ioctl.
#[derive(Debug)]
pub struct QBuffer<H: PlaneHandle> {
    pub flags: BufferFlags,
    pub field: u32,
    pub sequence: u32,
    pub planes: Vec<QBufPlane<H>>,
}

impl<H: PlaneHandle> Default for QBuffer<H> {
    fn default() -> Self {
        QBuffer {
            flags: Default::default(),
            field: Default::default(),
            sequence: Default::default(),
            planes: Vec::new(),
        }
    }
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
        H::fill_v4l2_buffer(&plane.handle, v4l2_buf);

        Ok(())
    }

    fn fill_mplane_v4l2_buffer(
        self,
        v4l2_buf: &mut bindings::v4l2_buffer,
        v4l2_planes: &mut PlaneData,
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
                H::fill_v4l2_plane(&plane.handle, v4l2_plane);
            });

        Ok(())
    }
}

#[doc(hidden)]
mod ioctl {
    use crate::bindings::v4l2_buffer;
    nix::ioctl_readwrite!(vidioc_querybuf, b'V', 9, v4l2_buffer);
    nix::ioctl_readwrite!(vidioc_qbuf, b'V', 15, v4l2_buffer);
    nix::ioctl_readwrite!(vidioc_dqbuf, b'V', 17, v4l2_buffer);
}

/// Safe wrapper around the `VIDIOC_QBUF` ioctl.
/// TODO: `qbuf` should be unsafe! The following invariants need to be guaranteed
/// by the caller:
///
/// For MMAP buffers, any mapping must not be accessed by the caller (or any
/// mapping must be unmapped before queueing?). Also if the buffer has been
/// DMABUF-exported, its consumers must likewise not access it.
///
/// For DMABUF buffers, the FD must not be duplicated and accessed anywhere else.
///
/// For USERPTR buffers, things are most tricky. Not only must the data not be
/// accessed by anyone else, the caller also needs to guarantee that the backing
/// memory won't be freed until the corresponding buffer is returned by either
/// `dqbuf` or `streamoff`.
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
        let mut plane_data: PlaneData = Default::default();
        v4l2_buf.m.planes = plane_data.as_mut_ptr();

        buf_data.fill_mplane_v4l2_buffer(&mut v4l2_buf, &mut plane_data)?;
        unsafe { ioctl::vidioc_qbuf(fd.as_raw_fd(), &mut v4l2_buf) }?;
        Ok(())
    } else {
        buf_data.fill_splane_v4l2_buffer(&mut v4l2_buf)?;
        unsafe { ioctl::vidioc_qbuf(fd.as_raw_fd(), &mut v4l2_buf) }?;
        Ok(())
    }
}

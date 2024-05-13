//! Safe wrapper for the VIDIOC_(D)QBUF and VIDIOC_QUERYBUF ioctls.
use nix::errno::Errno;
use nix::libc::{suseconds_t, time_t};
use nix::sys::time::{TimeVal, TimeValLike};
use std::fmt::Debug;
use std::os::unix::io::AsRawFd;
use std::os::unix::prelude::RawFd;
use thiserror::Error;

use crate::bindings;
use crate::bindings::v4l2_buffer;
use crate::ioctl::BufferFlags;
use crate::ioctl::QueryBuf;
use crate::ioctl::V4l2Buffer;
use crate::ioctl::V4l2BufferPlanes;
use crate::memory::Memory;
use crate::memory::PlaneHandle;
use crate::QueueType;

#[derive(Debug, Error)]
pub enum QBufError<Q: QueryBuf> {
    #[error("error while converting from v4l2_buffer: {0}")]
    ConversionError(Q::Error),
    #[error("invalid number of planes specified for the buffer: got {0}, expected {1}")]
    NumPlanesMismatch(usize, usize),
    #[error("data offset specified while using the single-planar API")]
    DataOffsetNotSupported,
    #[error("ioctl error: {0}")]
    IoctlError(Errno),
}

impl<Q: QueryBuf> From<Errno> for QBufError<Q> {
    fn from(errno: Errno) -> Self {
        Self::IoctlError(errno)
    }
}

impl<Q: QueryBuf> From<QBufError<Q>> for Errno {
    fn from(err: QBufError<Q>) -> Self {
        match err {
            QBufError::ConversionError(_) => Errno::EINVAL,
            QBufError::NumPlanesMismatch(_, _) => Errno::EINVAL,
            QBufError::DataOffsetNotSupported => Errno::EINVAL,
            QBufError::IoctlError(e) => e,
        }
    }
}

/// Implementors can pass buffer data to the `qbuf` ioctl.
pub trait QBuf<Q: QueryBuf> {
    /// Fill the buffer information into the single-planar `v4l2_buf`. Fail if
    /// the number of planes is different from 1.
    fn fill_splane_v4l2_buffer(self, v4l2_buf: &mut v4l2_buffer) -> Result<(), QBufError<Q>>;
    /// Fill the buffer information into the multi-planar `v4l2_buf`, using
    /// `v4l2_planes` to store the plane data. Fail if the number of planes is
    /// not between 1 and `VIDEO_MAX_PLANES` included.
    fn fill_mplane_v4l2_buffer(
        self,
        v4l2_buf: &mut v4l2_buffer,
        v4l2_planes: &mut V4l2BufferPlanes,
    ) -> Result<(), QBufError<Q>>;
}

impl<Q: QueryBuf> QBuf<Q> for V4l2Buffer {
    fn fill_splane_v4l2_buffer(self, v4l2_buf: &mut v4l2_buffer) -> Result<(), QBufError<Q>> {
        *v4l2_buf = self.buffer;

        Ok(())
    }

    fn fill_mplane_v4l2_buffer(
        self,
        v4l2_buf: &mut v4l2_buffer,
        v4l2_planes: &mut V4l2BufferPlanes,
    ) -> Result<(), QBufError<Q>> {
        *v4l2_buf = self.buffer;
        for (dest, src) in v4l2_planes
            .iter_mut()
            .zip(self.planes.iter())
            .take(v4l2_buf.length as usize)
        {
            *dest = *src;
        }

        Ok(())
    }
}

/// Representation of a single plane of a V4L2 buffer.
pub struct QBufPlane(pub bindings::v4l2_plane);

impl QBufPlane {
    // TODO remove as this is not safe - we should always specify a handle.
    pub fn new(bytes_used: usize) -> Self {
        QBufPlane(bindings::v4l2_plane {
            bytesused: bytes_used as u32,
            data_offset: 0,
            ..Default::default()
        })
    }

    pub fn new_from_handle<H: PlaneHandle>(handle: &H, bytes_used: usize) -> Self {
        let mut plane = Self::new(bytes_used);
        handle.fill_v4l2_plane(&mut plane.0);
        plane
    }
}

impl Debug for QBufPlane {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("QBufPlane")
            .field("bytesused", &self.0.bytesused)
            .field("data_offset", &self.0.data_offset)
            .finish()
    }
}

/// Contains all the information that can be passed to the `qbuf` ioctl.
// TODO Change this to contain a v4l2_buffer, and create constructors/methods
// to change it? Then during qbuf we just need to set m.planes to planes
// (after resizing it to 8) and we are good to use it as-is.
// We could even turn the trait into AsRef<v4l2_buffer> for good measure.
#[derive(Debug)]
pub struct QBuffer<H: PlaneHandle> {
    pub flags: BufferFlags,
    pub field: u32,
    pub sequence: u32,
    pub timestamp: TimeVal,
    pub planes: Vec<QBufPlane>,
    pub request: Option<RawFd>,
    pub _h: std::marker::PhantomData<H>,
}

impl<H: PlaneHandle> Default for QBuffer<H> {
    fn default() -> Self {
        QBuffer {
            flags: Default::default(),
            field: Default::default(),
            sequence: Default::default(),
            timestamp: TimeVal::zero(),
            planes: Vec::new(),
            request: None,
            _h: std::marker::PhantomData,
        }
    }
}

impl<H: PlaneHandle> QBuffer<H> {
    pub fn set_timestamp(mut self, sec: time_t, usec: suseconds_t) -> Self {
        self.timestamp = TimeVal::new(sec, usec);
        self
    }

    fn fill_common_v4l2_data(&self, v4l2_buf: &mut v4l2_buffer) {
        v4l2_buf.memory = H::Memory::MEMORY_TYPE as u32;
        v4l2_buf.flags = self.flags.bits();
        v4l2_buf.field = self.field;
        v4l2_buf.sequence = self.sequence;
        v4l2_buf.timestamp.tv_sec = self.timestamp.tv_sec();
        v4l2_buf.timestamp.tv_usec = self.timestamp.tv_usec();
        if let Some(request) = &self.request {
            v4l2_buf.__bindgen_anon_1.request_fd = *request;
        }
    }
}

impl<H: PlaneHandle, Q: QueryBuf> QBuf<Q> for QBuffer<H> {
    fn fill_splane_v4l2_buffer(self, v4l2_buf: &mut v4l2_buffer) -> Result<(), QBufError<Q>> {
        if self.planes.len() != 1 {
            return Err(QBufError::NumPlanesMismatch(self.planes.len(), 1));
        }

        self.fill_common_v4l2_data(v4l2_buf);

        let plane = &self.planes[0];
        if plane.0.data_offset != 0 {
            return Err(QBufError::DataOffsetNotSupported);
        }

        v4l2_buf.length = plane.0.length;
        v4l2_buf.bytesused = plane.0.bytesused;
        v4l2_buf.m = (&plane.0.m, H::Memory::MEMORY_TYPE).into();

        Ok(())
    }

    fn fill_mplane_v4l2_buffer(
        self,
        v4l2_buf: &mut v4l2_buffer,
        v4l2_planes: &mut V4l2BufferPlanes,
    ) -> Result<(), QBufError<Q>> {
        if self.planes.is_empty() || self.planes.len() > v4l2_planes.len() {
            return Err(QBufError::NumPlanesMismatch(
                self.planes.len(),
                v4l2_planes.len(),
            ));
        }

        self.fill_common_v4l2_data(v4l2_buf);

        v4l2_buf.length = self.planes.len() as u32;
        v4l2_planes
            .iter_mut()
            .zip(self.planes)
            .for_each(|(v4l2_plane, plane)| {
                *v4l2_plane = plane.0;
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
    nix::ioctl_readwrite!(vidioc_prepare_buf, b'V', 93, v4l2_buffer);
}

/// Safe wrapper around the `VIDIOC_QBUF` ioctl.
///
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
pub fn qbuf<I: QBuf<O>, O: QueryBuf>(
    fd: &impl AsRawFd,
    queue: QueueType,
    index: usize,
    buf_data: I,
) -> Result<O, QBufError<O>> {
    let mut v4l2_buf = v4l2_buffer {
        index: index as u32,
        type_: queue as u32,
        ..Default::default()
    };

    if queue.is_multiplanar() {
        let mut plane_data: V4l2BufferPlanes = Default::default();
        buf_data.fill_mplane_v4l2_buffer(&mut v4l2_buf, &mut plane_data)?;
        v4l2_buf.m.planes = plane_data.as_mut_ptr();

        unsafe { ioctl::vidioc_qbuf(fd.as_raw_fd(), &mut v4l2_buf) }?;
        Ok(O::try_from_v4l2_buffer(v4l2_buf, Some(plane_data))
            .map_err(QBufError::ConversionError)?)
    } else {
        buf_data.fill_splane_v4l2_buffer(&mut v4l2_buf)?;
        unsafe { ioctl::vidioc_qbuf(fd.as_raw_fd(), &mut v4l2_buf) }?;
        Ok(O::try_from_v4l2_buffer(v4l2_buf, None).map_err(QBufError::ConversionError)?)
    }
}

/// Safe wrapper around the `VIDIOC_PREPARE_BUF` ioctl.
pub fn prepare_buf<I: QBuf<O>, O: QueryBuf>(
    fd: &impl AsRawFd,
    queue: QueueType,
    index: usize,
    buf_data: I,
) -> Result<O, QBufError<O>> {
    let mut v4l2_buf = v4l2_buffer {
        index: index as u32,
        type_: queue as u32,
        ..Default::default()
    };

    if queue.is_multiplanar() {
        let mut plane_data: V4l2BufferPlanes = Default::default();
        buf_data.fill_mplane_v4l2_buffer(&mut v4l2_buf, &mut plane_data)?;
        v4l2_buf.m.planes = plane_data.as_mut_ptr();

        unsafe { ioctl::vidioc_prepare_buf(fd.as_raw_fd(), &mut v4l2_buf) }?;
        Ok(O::try_from_v4l2_buffer(v4l2_buf, Some(plane_data))
            .map_err(QBufError::ConversionError)?)
    } else {
        buf_data.fill_splane_v4l2_buffer(&mut v4l2_buf)?;
        unsafe { ioctl::vidioc_prepare_buf(fd.as_raw_fd(), &mut v4l2_buf) }?;
        Ok(O::try_from_v4l2_buffer(v4l2_buf, None).map_err(QBufError::ConversionError)?)
    }
}

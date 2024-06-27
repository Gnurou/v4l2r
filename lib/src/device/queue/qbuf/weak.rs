use super::super::{buffer::BufferInfo, Output};
use super::super::{BufferStateFuse, BuffersAllocated, QBuffer, Queue};
use crate::device::Device;
use crate::ioctl::{self, QBufResult};
use crate::memory::*;
use std::convert::Infallible;
use std::{
    os::fd::RawFd,
    sync::{Arc, Weak},
};

use nix::sys::time::TimeVal;

/// The weak version QBuffer without strong reference to its queue, therefore
/// it up to the application to ensure that the state of the queue or device
/// is not changed while the buffer is being used.
///
/// The QBufferWeak is espcially usefull for the frame based stateless decoder
/// when processing farmes with mutliple slices in the output direction.
///
/// The weak version of the output buffer can be obtained via `QBuffer::take()`
/// method called on previously allocated output buffer.
///
pub struct QBufferWeak<P: PrimitiveBufferHandles, Q: BufferHandles + From<P>> {
    device: Weak<Device>,
    buffer_info: Weak<BufferInfo<Q>>,
    timestamp: TimeVal,
    request: Option<RawFd>,
    fuse: BufferStateFuse<Q>,
    _p: std::marker::PhantomData<P>,
}

impl<P, Q> QBufferWeak<P, Q>
where
    P: PrimitiveBufferHandles,
    Q: BufferHandles + From<P>,
{
    /// Returns the V4L2 index of this buffer.
    pub fn index(&self) -> usize {
        self.buffer_info
            .upgrade()
            .expect("Failed to upgrade buffer info")
            .features
            .index
    }

    /// Returns the number of handles/plane data expected to be specified for
    /// this buffer.
    pub fn num_expected_planes(&self) -> usize {
        self.buffer_info
            .upgrade()
            .expect("Failed to upgrade buffer info")
            .features
            .planes
            .len()
    }

    pub fn set_timestamp(mut self, timestamp: TimeVal) -> Self {
        self.timestamp = timestamp;
        self
    }

    pub fn set_request(mut self, fd: RawFd) -> Self {
        self.request = Some(fd);
        self
    }
}

impl<P, Q> QBufferWeak<P, Q>
where
    P: PrimitiveBufferHandles,
    P::HandleType: Mappable,
    Q: BufferHandles + From<P>,
{
    pub fn get_plane_mapping(&self, plane_index: usize) -> Option<ioctl::PlaneMapping> {
        // We can only obtain a mapping if this buffer has not been deleted.
        let buffer_info = self.buffer_info.upgrade()?;
        let plane = buffer_info.features.planes.get(plane_index)?;
        // If the buffer info was alive, then the device must also be.
        let device = self.device.upgrade()?;
        P::HandleType::map(device.as_ref(), plane)
    }
}

impl<'a, P, Q> QBuffer<'a, Output, P, Q>
where
    P: PrimitiveBufferHandles,
    P::HandleType: Mappable,
    Q: BufferHandles + From<P>,
{
    pub fn take(self) -> QBufferWeak<P, Q> {
        QBufferWeak::<P, Q>::from(self)
    }
}

impl<P, Q> QBufferWeak<P, Q>
where
    P: PrimitiveBufferHandles + Default,
    Q: BufferHandles + From<P>,
    <P::HandleType as PlaneHandle>::Memory: SelfBacked,
{
    pub fn queue(
        self,
        bytes_used: &[usize],
        queue: &Queue<Output, BuffersAllocated<Q>>,
    ) -> QBufResult<(), Infallible> {
        QBuffer::<'_, Output, P, Q>::from((queue, self)).queue(bytes_used)
    }
}

impl<'a, P, Q> From<(&'a Queue<Output, BuffersAllocated<Q>>, QBufferWeak<P, Q>)>
    for QBuffer<'a, Output, P, Q>
where
    P: PrimitiveBufferHandles,
    Q: BufferHandles + From<P>,
{
    fn from(
        tuple: (&'a Queue<Output, BuffersAllocated<Q>>, QBufferWeak<P, Q>),
    ) -> QBuffer<'a, Output, P, Q> {
        let buffer_info = tuple
            .1
            .buffer_info
            .upgrade()
            .expect("Failed to upgrade buffer info");
        QBuffer {
            queue: tuple.0,
            index: buffer_info.features.index,
            num_planes: buffer_info.features.planes.len(),
            timestamp: tuple.1.timestamp,
            request: tuple.1.request,
            fuse: tuple.1.fuse,
            _p: tuple.1._p,
        }
    }
}

impl<'a, P, Q> From<QBuffer<'a, Output, P, Q>> for QBufferWeak<P, Q>
where
    P: PrimitiveBufferHandles,
    Q: BufferHandles + From<P>,
{
    fn from(qbuf: QBuffer<'a, Output, P, Q>) -> QBufferWeak<P, Q> {
        QBufferWeak {
            device: Arc::downgrade(&qbuf.queue.inner.device),
            buffer_info: Arc::downgrade(
                qbuf.queue
                    .state
                    .buffer_info
                    .get(qbuf.index)
                    .expect("Failed to get buffer info"),
            ),
            timestamp: qbuf.timestamp,
            request: qbuf.request,
            fuse: qbuf.fuse,
            _p: qbuf._p,
        }
    }
}

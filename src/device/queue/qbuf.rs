//! Provides types related to queuing buffers on a `Queue` object.
use super::{states::BufferInfo, Capture, Direction, Output};
use super::{BufferState, BufferStateFuse, BuffersAllocated, Queue};
use crate::ioctl;
use crate::memory::*;
use std::cmp::Ordering;
use std::{
    fmt::{self, Debug},
    sync::Arc,
};

use ioctl::{PlaneMapping, QBufError};
use thiserror::Error;

pub mod get_free;
pub mod get_indexed;

/// Error that can occur when queuing a buffer. It wraps a regular error and also
/// returns the plane handles back to the user.
#[derive(Error)]
#[error("{}", self.error)]
pub struct QueueError<P: PrimitiveBufferHandles> {
    pub error: QBufError,
    pub plane_handles: P,
}

impl<P: PrimitiveBufferHandles> Debug for QueueError<P> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        Debug::fmt(&self.error, f)
    }
}

#[allow(type_alias_bounds)]
pub type QueueResult<P: PrimitiveBufferHandles, R> = std::result::Result<R, QueueError<P>>;

/// A free buffer that has just been obtained from `Queue::get_buffer()` and
/// which is being prepared to the queued.
///
/// The necessary setup depends on the kind of direction of the buffer:
///
/// * Capture buffers are to be filled by the driver, so we just need to attach
///   one memory handle per plane before submitting them (MMAP buffers don't
///   need this step).
/// * Output buffers on the other hand are filled by us ; so on top of one valid
///   memory handle per plane, we also need to specify how much data we have
///   written in each of them, and possibly set a few flags on the buffer.
///
/// This struct is specialized on both the direction and type of memory so
/// mandatory data is always specified, and irrelevant data is inaccessible.
///
/// Once a buffer is ready, it can be queued using the queue() method. Failures
/// occur if the QBUF ioctl failed, or if the number of specified planes does
/// not match the number of planes in the format. A queued buffer remains
/// inaccessible for further queuing until it has been dequeued and dropped.
///
/// If a QBuffer object is destroyed before being queued, its buffer returns
/// to the pool of available buffers and can be requested again with
/// `Queue::get_buffer()`.
///
/// A QBuffer holds a strong reference to its queue, therefore the state of the
/// queue or device cannot be changed while it is being used. Contrary to
/// DQBuffer which can be freely duplicated and passed around, instances of this
/// struct are supposed to be short-lived.
pub struct QBuffer<'a, D: Direction, P: PrimitiveBufferHandles, Q: BufferHandles + From<P>> {
    queue: &'a Queue<D, BuffersAllocated<Q>>,
    index: usize,
    num_planes: usize,
    planes: Vec<Plane<D>>,
    fuse: BufferStateFuse<Q>,
    _p: std::marker::PhantomData<P>,
}

impl<'a, D: Direction, P: PrimitiveBufferHandles, Q: BufferHandles + From<P>> QBuffer<'a, D, P, Q> {
    pub(super) fn new(
        // TODO NOPE!! It should be BuffersAllocated<Q>!
        // And we should ask to create a buffer of type P!
        queue: &'a Queue<D, BuffersAllocated<Q>>,
        buffer_info: &BufferInfo<Q>,
    ) -> Self {
        let buffer = &buffer_info.features;
        let fuse = BufferStateFuse::new(Arc::downgrade(&buffer_info.state));

        QBuffer {
            queue,
            index: buffer.index,
            num_planes: buffer.planes.len(),
            planes: Default::default(),
            fuse,
            _p: std::marker::PhantomData,
        }
    }

    /// Returns the V4L2 index of this buffer.
    pub fn index(&self) -> usize {
        self.index
    }

    /// Returns the number of planes expected to be specified before this buffer
    /// can be queued.
    pub fn num_expected_planes(&self) -> usize {
        self.num_planes
    }

    /// Returns the number of planes that have been specified so far.
    pub fn num_set_planes(&self) -> usize {
        self.planes.len()
    }

    /// Specify the next plane of this buffer.
    /// TODO Take a Plane as argument, build using dedicated constructors for Output and Capture queues.
    pub fn add_plane(mut self, plane: Plane<D>) -> Self {
        self.planes.push(plane);
        self
    }

    // TODO QueueResult is backwards??
    fn queue_bound(mut self, plane_handles: P) -> QueueResult<P, ()> {
        // First check that the number of provided planes is what we expect.
        let num_planes = self.planes.len();
        match num_planes.cmp(&self.num_planes) {
            Ordering::Less => {
                return Err(QueueError {
                    error: QBufError::NotEnoughPlanes(num_planes, self.num_planes),
                    plane_handles,
                })
            }
            Ordering::Greater => {
                return Err(QueueError {
                    error: QBufError::TooManyPlanes(num_planes, self.num_planes),
                    plane_handles,
                })
            }
            Ordering::Equal => (),
        }

        let qbuffer = ioctl::QBuffer::<P::HandleType> {
            planes: self.planes.into_iter().map(|p| p.plane).collect(),
            ..Default::default()
        };

        match ioctl::qbuf(
            &self.queue.inner,
            self.queue.inner.type_,
            self.index,
            qbuffer,
        ) {
            Ok(_) => (),
            Err(error) => {
                return Err(QueueError {
                    error,
                    plane_handles,
                })
            }
        };

        // We got this now.
        self.fuse.disarm();

        let mut buffer_state = self
            .queue
            .state
            .buffer_info
            .get(self.index)
            .expect("Inconsistent buffer state!")
            .state
            .lock()
            .unwrap();
        *buffer_state = BufferState::Queued(plane_handles.into());
        drop(buffer_state);

        let num_queued_buffers = self.queue.state.num_queued_buffers.take();
        self.queue
            .state
            .num_queued_buffers
            .set(num_queued_buffers + 1);

        Ok(())
    }
}

impl<'a, D, P, Q> QBuffer<'a, D, P, Q>
where
    D: Direction,
    P: PrimitiveBufferHandles + Default,
    <P::HandleType as PlaneHandle>::Memory: SelfBacked,
    Q: BufferHandles + From<P>,
{
    /// Queue a self-backed buffer that does not need handles. The QBuffer
    /// object is consumed and the buffer won't be available again until it is
    /// dequeued and dropped, or a `streamoff()` is performed.
    pub fn queue(self) -> QueueResult<P, ()> {
        self.queue_bound(Default::default())
    }
}

impl<'a, D, P, Q> QBuffer<'a, D, P, Q>
where
    D: Direction,
    P: PrimitiveBufferHandles,
    Q: BufferHandles + From<P>,
{
    /// Queue the buffer after binding `plane_handles`. The QBuffer object is
    /// consumed and the buffer won't be available again until it is dequeued
    /// and dropped, or a `streamoff()` is performed.
    pub fn queue_with_handles(mut self, plane_handles: P) -> QueueResult<P, ()> {
        // Check that we have provided the right number of handles for our planes.
        let num_plane_handles = plane_handles.len();
        match num_plane_handles.cmp(&self.num_planes) {
            Ordering::Less => {
                return Err(QueueError {
                    error: QBufError::NotEnoughPlanes(num_plane_handles, self.num_planes),
                    plane_handles,
                })
            }
            Ordering::Greater => {
                return Err(QueueError {
                    error: QBufError::TooManyPlanes(num_plane_handles, self.num_planes),
                    plane_handles,
                })
            }
            Ordering::Equal => (),
        };

        for (index, plane) in self.planes.iter_mut().enumerate() {
            // TODO take the QBufPlane as argument if possible?
            plane_handles.fill_v4l2_plane(index, &mut plane.plane.0);
        }

        self.queue_bound(plane_handles)
    }
}

impl<'a, P, Q> QBuffer<'a, Output, P, Q>
where
    P: PrimitiveBufferHandles,
    P::HandleType: Mappable,
    Q: BufferHandles + From<P>,
{
    pub fn get_plane_mapping(&self, plane: usize) -> Option<PlaneMapping> {
        let buffer_info = self.queue.state.buffer_info.get(self.index)?;
        let plane_info = buffer_info.features.planes.get(plane)?;
        P::HandleType::map(self.queue.inner.device.as_ref(), plane_info)
    }
}

/// Used to build plane information for a buffer about to be queued. This
/// struct is specialized on direction and buffer type to only the relevant
/// data can be set according to the current context.
pub struct Plane<D: Direction> {
    plane: ioctl::QBufPlane,
    _d: std::marker::PhantomData<D>,
}

impl Plane<Capture> {
    /// Creates a new plane suitable for a bound capture queue.
    pub fn cap() -> Self {
        Self {
            plane: ioctl::QBufPlane::new(0),
            _d: std::marker::PhantomData,
        }
    }
}

impl Plane<Output> {
    /// Creates a new plane builder suitable for an output queue.
    /// Mandatory information include the number of bytes used.
    pub fn out(bytes_used: usize) -> Self {
        Self {
            plane: ioctl::QBufPlane::new(bytes_used),
            _d: std::marker::PhantomData,
        }
    }
}

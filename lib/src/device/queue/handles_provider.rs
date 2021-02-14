use std::{
    collections::VecDeque,
    fmt::Debug,
    sync::{Arc, Mutex, Weak},
};

use log::error;

use crate::{
    bindings,
    device::poller::Waker,
    memory::{BufferHandles, MMAPHandle, PrimitiveBufferHandles},
    Format,
};

use thiserror::Error;

use super::qbuf::{
    get_free::{GetFreeBufferError, GetFreeCaptureBuffer},
    get_indexed::{GetCaptureBufferByIndex, TryGetBufferError},
    CaptureQueueableProvider,
};

#[derive(Debug, Error)]
pub enum GetSuitableBufferError {
    #[error("Error while calling try_get_free_buffer()")]
    TryGetFree(#[from] GetFreeBufferError),
    #[error("Error while calling try_get_buffer()")]
    TryGetIndexed(#[from] TryGetBufferError),
}

pub trait HandlesProvider: Send + 'static {
    type HandleType: BufferHandles;

    /// Request a set of handles. Returns `None` if no handle is currently
    /// available. If that is the case, `waker` will be signaled when handles
    /// are available again.
    fn get_handles(&mut self, waker: &Arc<Waker>) -> Option<Self::HandleType>;

    fn get_suitable_buffer_for<'a, Q>(
        &self,
        _handles: &Self::HandleType,
        queue: &'a Q,
    ) -> Result<
        <Q as CaptureQueueableProvider<'a, Self::HandleType>>::Queueable,
        GetSuitableBufferError,
    >
    where
        Q: GetCaptureBufferByIndex<'a, Self::HandleType>
            + GetFreeCaptureBuffer<'a, Self::HandleType>,
    {
        Ok(queue.try_get_free_buffer()?)
    }
}

/// `HandleProvider`s on the heap are `HandleProvider`s too.
impl<P: HandlesProvider> HandlesProvider for Box<P> {
    type HandleType = P::HandleType;

    fn get_handles(&mut self, waker: &Arc<Waker>) -> Option<Self::HandleType> {
        self.as_mut().get_handles(waker)
    }

    fn get_suitable_buffer_for<'a, Q>(
        &self,
        handles: &Self::HandleType,
        queue: &'a Q,
    ) -> Result<
        <Q as CaptureQueueableProvider<'a, Self::HandleType>>::Queueable,
        GetSuitableBufferError,
    >
    where
        Q: GetCaptureBufferByIndex<'a, Self::HandleType>
            + GetFreeCaptureBuffer<'a, Self::HandleType>,
    {
        self.as_ref().get_suitable_buffer_for(handles, queue)
    }
}

pub struct MMAPProvider(Vec<MMAPHandle>);

impl MMAPProvider {
    pub fn new(format: &Format) -> Self {
        Self(vec![Default::default(); format.plane_fmt.len()])
    }
}

impl HandlesProvider for MMAPProvider {
    type HandleType = Vec<MMAPHandle>;

    fn get_handles(&mut self, _waker: &Arc<Waker>) -> Option<Self::HandleType> {
        Some(self.0.clone())
    }
}

/// Internals of `PooledHandlesProvider`, which acts just as a protected wrapper
/// around this structure.
struct PooledHandlesProviderInternal<H: BufferHandles> {
    buffers: VecDeque<H>,
    waker: Option<Arc<Waker>>,
}

unsafe impl<H: BufferHandles> Send for PooledHandlesProviderInternal<H> {}

/// A handles provider that recycles buffers from a fixed set in a pool.
/// Provided `PooledHandles` will not be recycled for as long as the instance is
/// alive. Once it is dropped, it the underlying buffer returns into the pool to
/// be reused later.
pub struct PooledHandlesProvider<H: BufferHandles> {
    d: Arc<Mutex<PooledHandlesProviderInternal<H>>>,
}

impl<H: BufferHandles> PooledHandlesProvider<H> {
    /// Create a new `PooledMemoryProvider`, using the set in `buffers`.
    pub fn new<B: IntoIterator<Item = H>>(buffers: B) -> Self {
        Self {
            d: Arc::new(Mutex::new(PooledHandlesProviderInternal {
                buffers: buffers.into_iter().collect(),
                waker: None,
            })),
        }
    }
}

impl<H: BufferHandles> HandlesProvider for PooledHandlesProvider<H> {
    type HandleType = PooledHandles<H>;

    fn get_handles(&mut self, waker: &Arc<Waker>) -> Option<PooledHandles<H>> {
        let mut d = self.d.lock().unwrap();
        match d.buffers.pop_front() {
            Some(handles) => Some(PooledHandles::new(&self.d, handles)),
            None => {
                d.waker = Some(Arc::clone(waker));
                None
            }
        }
    }
}

/// A set of buffer handles provided by `PooledHandlesProvider`. The handles
/// will remain out of the pool as long as this instance is alive, i.e. the
/// handles will be recycled when it is dropped.
pub struct PooledHandles<H: BufferHandles> {
    // Use of Option is necessary here because of Drop implementation, but the
    // Option will always be Some()
    handles: Option<H>,
    provider: Weak<Mutex<PooledHandlesProviderInternal<H>>>,
}

impl<H: BufferHandles> PooledHandles<H> {
    fn new(provider: &Arc<Mutex<PooledHandlesProviderInternal<H>>>, handles: H) -> Self {
        Self {
            handles: Some(handles),
            provider: Arc::downgrade(provider),
        }
    }

    pub fn handles(&self) -> &H {
        self.handles.as_ref().unwrap()
    }
}

impl<H: BufferHandles + Debug> Debug for PooledHandles<H> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.handles.fmt(f)
    }
}

impl<H: BufferHandles> Drop for PooledHandles<H> {
    /// Return the handles to the pool if it still exists, otherwise the handles
    /// themselves are destroyed.
    fn drop(&mut self) {
        match self.provider.upgrade() {
            None => (),
            Some(provider) => {
                let mut provider = provider.lock().unwrap();
                provider.buffers.push_back(self.handles.take().unwrap());
                if let Some(waker) = provider.waker.take() {
                    waker.wake().unwrap_or_else(|e| {
                        error!("Error signaling waker after PooledHandles drop: {}", e);
                    });
                }
            }
        }
    }
}

impl<H: BufferHandles> BufferHandles for PooledHandles<H> {
    type SupportedMemoryType = H::SupportedMemoryType;

    fn len(&self) -> usize {
        self.handles.as_ref().unwrap().len()
    }

    fn fill_v4l2_plane(&self, index: usize, plane: &mut bindings::v4l2_plane) {
        self.handles.as_ref().unwrap().fill_v4l2_plane(index, plane);
    }
}

impl<H: PrimitiveBufferHandles> PrimitiveBufferHandles for PooledHandles<H> {
    type HandleType = H::HandleType;
    const MEMORY_TYPE: Self::SupportedMemoryType = H::MEMORY_TYPE;
}

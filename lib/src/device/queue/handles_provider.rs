use std::{
    collections::VecDeque,
    fmt::Debug,
    sync::{Arc, Mutex, Weak},
};

use crate::{
    bindings,
    memory::{BufferHandles, MMAPHandle, PrimitiveBufferHandles},
    Format,
};

pub trait HandlesProvider: Send + 'static {
    type HandleType: BufferHandles;

    fn get_handles(&mut self) -> Option<Self::HandleType>;
}

pub struct MMAPProvider(Vec<MMAPHandle>);

impl MMAPProvider {
    pub fn new(format: &Format) -> Self {
        Self(vec![Default::default(); format.plane_fmt.len()])
    }
}

impl HandlesProvider for MMAPProvider {
    type HandleType = Vec<MMAPHandle>;

    fn get_handles(&mut self) -> Option<Self::HandleType> {
        Some(self.0.clone())
    }
}

/// A handles provider that recycles buffers from a fixed set in a pool.
/// Provided `PooledHandles` will not be recycled for as long as the instance is
/// alive. Once it is dropped, it the underlying buffer returns into the pool to
/// be reused later.
pub struct PooledHandlesProvider<H: BufferHandles> {
    buffers: Arc<Mutex<VecDeque<H>>>,
}

impl<H: BufferHandles> PooledHandlesProvider<H> {
    /// Create a new `PooledMemoryProvider`, using the set in `buffers`.
    pub fn new<B: IntoIterator<Item = H>>(buffers: B) -> Self {
        Self {
            buffers: Arc::new(Mutex::new(buffers.into_iter().collect())),
        }
    }
}

impl<H: BufferHandles> HandlesProvider for PooledHandlesProvider<H> {
    type HandleType = PooledHandles<H>;

    fn get_handles(&mut self) -> Option<PooledHandles<H>> {
        let mut buffers = self.buffers.lock().unwrap();
        match buffers.pop_front() {
            Some(handles) => Some(PooledHandles::new(&self.buffers, handles)),
            None => None,
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
    provider: Weak<Mutex<VecDeque<H>>>,
}

impl<H: BufferHandles> PooledHandles<H> {
    fn new(provider: &Arc<Mutex<VecDeque<H>>>, handles: H) -> Self {
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
                let mut buffers = provider.lock().unwrap();
                buffers.push_back(self.handles.take().unwrap());
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

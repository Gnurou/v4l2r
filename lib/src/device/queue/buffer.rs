use super::BufferHandles;
use crate::ioctl;

use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex,
};

/// Represents the current state of an allocated buffer.
pub(super) enum BufferState<P: BufferHandles> {
    /// The buffer can be obtained via `get_buffer()` and be queued.
    Free,
    /// The buffer has been requested via `get_buffer()` but is not queued yet.
    PreQueue,
    /// The buffer is queued and waiting to be dequeued.
    Queued(P),
    /// The buffer has been dequeued and the client is still using it. The buffer
    /// will go back to the `Free` state once the reference is dropped.
    Dequeued,
}

/// Structure that allows a queue and its users to keep track of how many buffers are available for
/// use and currently queued.
#[derive(Default)]
pub(super) struct BufferStats {
    num_free: AtomicUsize,
    num_queued: AtomicUsize,
}

impl BufferStats {
    /// Create a new tracker for buffer stats. The stats are initially empty, so this structure
    /// must be passed to `BufferInfo::new` for the buffer to be initially registered.
    pub fn new() -> Self {
        Self {
            num_free: AtomicUsize::new(0),
            num_queued: AtomicUsize::new(0),
        }
    }

    pub fn num_free(&self) -> usize {
        self.num_free.load(Ordering::Relaxed)
    }

    pub fn num_queued(&self) -> usize {
        self.num_queued.load(Ordering::Relaxed)
    }
}

pub(super) struct BufferInfo<P: BufferHandles> {
    /// Static information about the buffer, obtains from V4L2's `QUERYBUF` ioctl.
    pub(super) features: ioctl::QueryBuffer,
    /// Current state of the buffer.
    state: Mutex<BufferState<P>>,
    /// Link to the queue's buffer stats, so we can update them as the buffer state changes.
    stats: Arc<BufferStats>,
}

impl<P: BufferHandles> Drop for BufferInfo<P> {
    fn drop(&mut self) {
        self.stats.num_free.fetch_sub(1, Ordering::Relaxed);
    }
}

impl<P: BufferHandles> BufferInfo<P> {
    pub(super) fn new(features: ioctl::QueryBuffer, stats: Arc<BufferStats>) -> Self {
        stats.num_free.fetch_add(1, Ordering::Relaxed);
        Self {
            state: Mutex::new(BufferState::Free),
            features,
            stats: Arc::clone(&stats),
        }
    }

    /// Do something with the buffer's state. The state is provided read-only and thus cannot be
    /// modified.
    pub(super) fn do_with_state<R, F: FnOnce(&BufferState<P>) -> R>(&self, f: F) -> R {
        f(&*self.state.lock().unwrap())
    }

    /// Update the buffer's state. The queue's stats will be updated to reflect the new state
    /// decided by `f`.
    pub(super) fn update_state<R, F: FnOnce(&mut BufferState<P>) -> R>(&self, f: F) -> R {
        let mut state = self.state.lock().unwrap();
        match *state {
            BufferState::Free => self.stats.num_free.fetch_sub(1, Ordering::Relaxed),
            BufferState::Queued(_) => self.stats.num_queued.fetch_sub(1, Ordering::Relaxed),
            _ => 0,
        };

        // Let the provided closure decide the new state.
        let res = f(&mut *state);

        match *state {
            BufferState::Free => self.stats.num_free.fetch_add(1, Ordering::Relaxed),
            BufferState::Queued(_) => self.stats.num_queued.fetch_add(1, Ordering::Relaxed),
            _ => 0,
        };

        res
    }
}

#[cfg(test)]
mod tests {
    use crate::memory::MmapHandle;

    use super::*;

    #[test]
    fn test_buffer_state_update() {
        const NUM_BUFFERS: usize = 5;

        let buffer_stats = Arc::new(BufferStats::new());
        assert_eq!(buffer_stats.num_free(), 0);
        assert_eq!(buffer_stats.num_queued(), 0);

        let buffers = (0..NUM_BUFFERS)
            .into_iter()
            .map(|i| {
                let querybuf = ioctl::QueryBuffer {
                    index: i,
                    flags: ioctl::BufferFlags::empty(),
                    planes: Default::default(),
                };
                let buffer: BufferInfo<Vec<MmapHandle>> =
                    BufferInfo::new(querybuf, Arc::clone(&buffer_stats));
                assert_eq!(buffer_stats.num_free(), i + 1);
                assert_eq!(buffer_stats.num_queued(), 0);

                buffer
            })
            .collect::<Vec<BufferInfo<Vec<MmapHandle>>>>();

        buffers[0].update_state(|s| {
            let _ = std::mem::replace(&mut *s, BufferState::PreQueue);
        });
        assert_eq!(buffer_stats.num_free(), NUM_BUFFERS - 1);
        assert_eq!(buffer_stats.num_queued(), 0);

        buffers[1].update_state(|s| {
            let _ = std::mem::replace(&mut *s, BufferState::PreQueue);
        });
        assert_eq!(buffer_stats.num_free(), NUM_BUFFERS - 2);
        assert_eq!(buffer_stats.num_queued(), 0);

        buffers[0].update_state(|s| {
            let _ = std::mem::replace(&mut *s, BufferState::Queued(Default::default()));
        });
        assert_eq!(buffer_stats.num_free(), NUM_BUFFERS - 2);
        assert_eq!(buffer_stats.num_queued(), 1);

        buffers[2].update_state(|s| {
            let _ = std::mem::replace(&mut *s, BufferState::Queued(Default::default()));
        });
        assert_eq!(buffer_stats.num_free(), NUM_BUFFERS - 3);
        assert_eq!(buffer_stats.num_queued(), 2);

        buffers[2].update_state(|s| {
            let _ = std::mem::replace(&mut *s, BufferState::Free);
        });
        assert_eq!(buffer_stats.num_free(), NUM_BUFFERS - 2);
        assert_eq!(buffer_stats.num_queued(), 1);

        buffers[0].update_state(|s| {
            let _ = std::mem::replace(&mut *s, BufferState::Dequeued);
        });
        assert_eq!(buffer_stats.num_free(), NUM_BUFFERS - 2);
        assert_eq!(buffer_stats.num_queued(), 0);

        buffers[0].update_state(|s| {
            let _ = std::mem::replace(&mut *s, BufferState::Free);
        });
        assert_eq!(buffer_stats.num_free(), NUM_BUFFERS - 1);
        assert_eq!(buffer_stats.num_queued(), 0);

        buffers[1].update_state(|s| {
            let _ = std::mem::replace(&mut *s, BufferState::Free);
        });
        assert_eq!(buffer_stats.num_free(), NUM_BUFFERS);
        assert_eq!(buffer_stats.num_queued(), 0);
    }
}

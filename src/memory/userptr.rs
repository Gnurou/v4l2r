//! Operations specific to UserPtr-type buffers.
use std::{
    collections::VecDeque,
    mem,
    sync::{Arc, Mutex, Weak},
};

use super::*;
use crate::{bindings, Format};

pub struct UserPtr;

impl Memory for UserPtr {
    const MEMORY_TYPE: MemoryType = MemoryType::UserPtr;
}

impl Imported for UserPtr {}

/// Handle for a USERPTR plane. These buffers are backed by userspace-allocated
/// memory, which translates well into Rust's slice of `u8`s. Since slices also
/// carry size information, we know that we are not passing unallocated areas
/// of the address-space to the kernel.
///
/// USERPTR buffers have the particularity that the `length` field of `struct
/// v4l2_buffer` must be set before doing a `QBUF` ioctl. This handle struct
/// also takes care of that.
#[derive(Debug)]
pub struct UserPtrHandle<T: AsRef<[u8]> + Debug + Send>(pub T);

impl<T: AsRef<[u8]> + Debug + Send> From<T> for UserPtrHandle<T> {
    fn from(buffer: T) -> Self {
        UserPtrHandle(buffer)
    }
}

impl<T: AsRef<[u8]> + Debug + Send> PlaneHandle for UserPtrHandle<T> {
    type Memory = UserPtr;

    fn fill_v4l2_plane(&self, plane: &mut bindings::v4l2_plane) {
        let slice = AsRef::<[u8]>::as_ref(&self.0);

        plane.m.userptr = slice.as_ptr() as std::os::raw::c_ulong;
        plane.length = slice.len() as u32;
    }

    fn fill_v4l2_splane_buffer(plane: &bindings::v4l2_plane, buffer: &mut bindings::v4l2_buffer) {
        buffer.m.userptr = unsafe { plane.m.userptr };
        buffer.length = plane.length;
    }
}

type UserBufferHandles = Vec<UserPtrHandle<Vec<u8>>>;

pub struct PooledUserMemoryProvider {
    num_buffers: usize,
    max_buffers: usize,
    format: Format,
    buffers: Arc<Mutex<VecDeque<UserBufferHandles>>>,
}

impl PooledUserMemoryProvider {
    pub fn new(max_buffers: usize, format: Format) -> Self {
        Self {
            num_buffers: 0,
            max_buffers,
            format,
            buffers: Arc::new(Mutex::new(VecDeque::with_capacity(max_buffers))),
        }
    }

    fn allocate_handles(format: &Format) -> UserBufferHandles {
        format
            .plane_fmt
            .iter()
            .map(|p| UserPtrHandle(vec![0u8; p.sizeimage as usize]))
            .collect()
    }
}

impl super::HandlesProvider for PooledUserMemoryProvider {
    type HandleType = PooledUserBuffer;

    fn get_handles(&mut self) -> Option<PooledUserBuffer> {
        if self.num_buffers >= self.max_buffers {
            return None;
        }

        let mut buffers = self.buffers.lock().unwrap();
        let handles = match buffers.pop_front() {
            Some(handles) => handles,
            None => {
                self.num_buffers += 1;
                Self::allocate_handles(&self.format)
            }
        };

        Some(PooledUserBuffer::new(&self.buffers, handles))
    }
}

pub struct PooledUserBuffer {
    handles: Vec<UserPtrHandle<Vec<u8>>>,
    provider: Weak<Mutex<VecDeque<UserBufferHandles>>>,
}

impl PooledUserBuffer {
    fn new(provider: &Arc<Mutex<VecDeque<UserBufferHandles>>>, handles: UserBufferHandles) -> Self {
        Self {
            handles,
            provider: Arc::downgrade(provider),
        }
    }
}

impl Debug for PooledUserBuffer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "PooledUserBuffer with {} planes", self.handles.len())
    }
}

impl Drop for PooledUserBuffer {
    // Return the planes to the pool if it still exists.
    fn drop(&mut self) {
        match self.provider.upgrade() {
            None => (),
            Some(provider) => {
                let mut buffers = provider.lock().unwrap();
                buffers.push_back(mem::take(&mut self.handles));
            }
        }
    }
}

impl BufferHandles for PooledUserBuffer {
    type SupportedMemoryType = MemoryType;

    fn len(&self) -> usize {
        self.handles.len()
    }

    fn fill_v4l2_plane(&self, index: usize, plane: &mut bindings::v4l2_plane) {
        self.handles[index].fill_v4l2_plane(plane);
    }
}

impl PrimitiveBufferHandles for PooledUserBuffer {
    type HandleType = UserPtrHandle<Vec<u8>>;
    const MEMORY_TYPE: Self::SupportedMemoryType = MemoryType::UserPtr;
}

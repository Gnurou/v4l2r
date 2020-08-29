//! Operations specific to UserPtr-type buffers.
use super::*;
use crate::bindings;

/// Handle for a USERPTR buffer. These buffers are backed by userspace-allocated
/// memory, which translates well into Rust's slice of `u8`s. Since slice also
/// carry size information, we know that we are not passing unallocated areas
/// of the address-space to the kernel.
///
/// USERPTR buffers have the particularity that the `length` field of `struct
/// v4l2_buffer` must be set before doing a `QBUF` ioctl. This handle struct
/// also takes care of that.
impl<T: AsRef<[u8]> + Debug> PlaneHandle for T {
    const MEMORY_TYPE: MemoryType = MemoryType::UserPtr;

    fn fill_v4l2_buffer(plane: &bindings::v4l2_plane, buffer: &mut bindings::v4l2_buffer) {
        buffer.m.userptr = unsafe { plane.m.userptr };
        buffer.length = plane.length;
    }

    fn fill_v4l2_plane(&self, plane: &mut bindings::v4l2_plane) {
        let slice = AsRef::<[u8]>::as_ref(self);

        plane.m.userptr = slice.as_ptr() as std::os::raw::c_ulong;
        plane.length = slice.len() as u32;
    }
}

/// A USERPTR buffer is always backed by userspace-allocated memory. We get this
/// memory through any kind of object that implements `AsRef<[u8]>`.
pub struct UserPtr<T: AsRef<[u8]>> {
    _t: std::marker::PhantomData<T>,
}

/// USERPTR buffers support for queues. We must guarantee that the
/// userspace-allocated memory will be alive and untouched until the buffer is
/// dequeued, so for this reason we take full ownership of it during `qbuf`,
/// and return it when the buffer is dequeued or the queue is stopped.
impl<T: AsRef<[u8]> + Debug> Memory for UserPtr<T> {
    type HandleType = T;
}

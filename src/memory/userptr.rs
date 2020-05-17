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
#[derive(Debug)]
pub struct UserPtrHandle {
    ptr: *const u8,
    length: u32,
}

impl UserPtrHandle {
    /// Create a new handle from anything that references bytes.
    ///
    /// This method is unsafe. The caller must guarantee that the owner of the
    /// buffer memory will outlive the created handle: this means keeping the
    /// owning object alive until the queued buffer using the handle has been
    /// dequeued or the queue streamed off.
    pub unsafe fn new<T: AsRef<[u8]>>(b: &T) -> Self {
        let slice = AsRef::<[u8]>::as_ref(b);

        UserPtrHandle {
            ptr: slice.as_ptr(),
            length: slice.len() as u32,
        }
    }
}

impl PlaneHandle for UserPtrHandle {
    const MEMORY_TYPE: MemoryType = MemoryType::UserPtr;

    fn fill_v4l2_buffer(&self, buffer: &mut bindings::v4l2_buffer) {
        buffer.m.userptr = self.ptr as std::os::raw::c_ulong;
        buffer.length = self.length as u32;
    }

    fn fill_v4l2_plane(&self, plane: &mut bindings::v4l2_plane) {
        plane.m.userptr = self.ptr as std::os::raw::c_ulong;
        plane.length = self.length as u32;
    }
}

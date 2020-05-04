//! Operations specific to UserPtr-type buffers.
use crate::bindings;
use crate::{MemoryType, PlaneHandle};

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
    pub ptr: *const u8,
    pub length: u32,
}

impl Default for UserPtrHandle {
    fn default() -> Self {
        UserPtrHandle {
            ptr: 0 as *const u8,
            length: 0,
        }
    }
}

impl From<&[u8]> for UserPtrHandle {
    fn from(slice: &[u8]) -> Self {
        UserPtrHandle {
            ptr: slice.as_ptr(),
            length: slice.len() as u32,
        }
    }
}

impl PlaneHandle for UserPtrHandle {
    const MEMORY_TYPE: MemoryType = MemoryType::UserPtr;

    unsafe fn from_v4l2_buffer(buffer: &bindings::v4l2_buffer) -> Self {
        UserPtrHandle {
            ptr: buffer.m.userptr as *const u8,
            length: buffer.length,
        }
    }

    unsafe fn from_v4l2_plane(plane: &bindings::v4l2_plane) -> Self {
        UserPtrHandle {
            ptr: plane.m.userptr as *const u8,
            length: plane.length,
        }
    }

    fn fill_v4l2_buffer(&self, buffer: &mut bindings::v4l2_buffer) {
        buffer.m.userptr = self.ptr as std::os::raw::c_ulong;
        buffer.length = self.length;
    }

    fn fill_v4l2_plane(&self, plane: &mut bindings::v4l2_plane) {
        plane.m.userptr = self.ptr as std::os::raw::c_ulong;
        plane.length = self.length;
    }
}

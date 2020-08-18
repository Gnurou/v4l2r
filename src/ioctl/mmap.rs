use crate::Result;
use std::os::unix::io::AsRawFd;
use std::{ops::Deref, slice};

use nix::{
    libc::{c_void, off_t, size_t},
    sys::mman,
};

pub struct PlaneMapping {
    // A mapping remains valid until we munmap it, that is, until the
    // PlaneMapping object is deleted. Hence the static lifetime.
    pub data: &'static mut [u8],
}

impl AsRef<[u8]> for PlaneMapping {
    fn as_ref(&self) -> &[u8] {
        self.data
    }
}

/// To provide len() and is_empty().
impl Deref for PlaneMapping {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        self.data
    }
}

impl Drop for PlaneMapping {
    fn drop(&mut self) {
        // Safe because the pointer and length were constructed in mmap() and
        // are always valid.
        unsafe { mman::munmap(self.data.as_mut_ptr() as *mut c_void, self.data.len()) }
            .unwrap_or_else(|e| {
                eprintln!("Error while unmapping plane: {}", e);
            });
    }
}

// TODO should be unsafe because the mapping can be used after a buffer is queued?
// Or not, since this cannot cause a crash...
pub fn mmap<F: AsRawFd>(fd: &F, mem_offset: u32, length: u32) -> Result<PlaneMapping> {
    let data = unsafe {
        mman::mmap(
            std::ptr::null_mut::<c_void>(),
            length as size_t,
            mman::ProtFlags::PROT_READ | mman::ProtFlags::PROT_WRITE,
            mman::MapFlags::MAP_SHARED,
            fd.as_raw_fd(),
            mem_offset as off_t,
        )
    }?;

    Ok(PlaneMapping {
        // Safe because we know the pointer is valid and has enough data mapped
        // to cover the length.
        data: unsafe { slice::from_raw_parts_mut(data as *mut u8, length as usize) },
    })
}

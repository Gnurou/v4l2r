use crate::Result;
use std::os::unix::io::AsRawFd;
use std::{
    ops::{Deref, Index, IndexMut},
    slice,
};

use nix::{
    libc::{c_void, off_t, size_t},
    sys::mman,
};

pub struct PlaneMapping<'a> {
    pub data: &'a mut [u8],
}

impl<'a> AsRef<[u8]> for PlaneMapping<'a> {
    fn as_ref(&self) -> &[u8] {
        self.data
    }
}

/// To provide len() and is_empty().
impl<'a> Deref for PlaneMapping<'a> {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        self.data
    }
}

impl<'a> Drop for PlaneMapping<'a> {
    fn drop(&mut self) {
        // Safe because the pointer and length were constructed in mmap() and
        // are always valid.
        unsafe { mman::munmap(self.data.as_mut_ptr() as *mut c_void, self.data.len()) }
            .unwrap_or_else(|e| {
                eprintln!("Error while unmapping plane: {}", e);
            });
    }
}

impl<'a> Index<usize> for PlaneMapping<'a> {
    type Output = u8;
    fn index(&self, index: usize) -> &Self::Output {
        &self.data[index]
    }
}

impl<'a> IndexMut<usize> for PlaneMapping<'a> {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        &mut self.data[index]
    }
}

// TODO should be unsafe because the mapping can be used after a buffer is queued?
// Or not, since this cannot cause a crash...
pub fn mmap<'a, F: AsRawFd>(fd: &F, mem_offset: u32, length: u32) -> Result<PlaneMapping<'a>> {
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

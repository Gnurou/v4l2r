use std::fs::File;

use dma_heap::{Heap, HeapKind};
use v4l2r::{memory::DmaBufHandle, Format};

use anyhow::Result;

pub fn export_dmabufs(format: &Format, nb_buffers: usize) -> Result<Vec<Vec<DmaBufHandle<File>>>> {
    let heap = Heap::new(HeapKind::System)?;

    let fds: Vec<Vec<DmaBufHandle<File>>> = (0..nb_buffers)
        .map(|_| {
            format
                .plane_fmt
                .iter()
                .map(|plane| {
                    let fd = File::from(heap.allocate(plane.sizeimage as usize).unwrap());
                    DmaBufHandle::from(fd)
                })
                .collect()
        })
        .collect();

    Ok(fds)
}

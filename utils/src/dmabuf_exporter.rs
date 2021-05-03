use std::fs::File;

use dma_heap::{DmaBufHeap, DmaBufHeapType};
use v4l2r::{memory::DmaBufHandle, Format};

use anyhow::Result;

pub fn export_dmabufs(format: &Format, nb_buffers: usize) -> Result<Vec<Vec<DmaBufHandle<File>>>> {
    let heap = DmaBufHeap::new(DmaBufHeapType::System).unwrap();

    let fds: Vec<Vec<DmaBufHandle<File>>> = (0..nb_buffers)
        .into_iter()
        .map(|_| {
            format
                .plane_fmt
                .iter()
                .map(|plane| {
                    DmaBufHandle::from(heap.allocate::<File>(plane.sizeimage as usize).unwrap())
                })
                .collect()
        })
        .collect();

    Ok(fds)
}

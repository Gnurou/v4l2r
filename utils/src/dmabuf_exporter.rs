use std::fs::File;

use nix::{
    sys::memfd::{memfd_create, MemFdCreateFlag},
    unistd::ftruncate,
};
use v4l2r::{memory::DmaBufHandle, Format};

use anyhow::Result;

pub fn export_dmabufs(format: &Format, nb_buffers: usize) -> Result<Vec<Vec<DmaBufHandle<File>>>> {
    let fds: Vec<Vec<DmaBufHandle<File>>> = (0..nb_buffers)
        .map(|_| {
            format
                .plane_fmt
                .iter()
                .map(|plane| {
                    memfd_create(c"memfd buffer", MemFdCreateFlag::MFD_ALLOW_SEALING)
                        .and_then(|fd| ftruncate(&fd, i64::from(plane.sizeimage)).map(|_| fd))
                        .map(|fd| DmaBufHandle::from(File::from(fd)))
                        .unwrap()
                })
                .collect()
        })
        .collect();

    Ok(fds)
}

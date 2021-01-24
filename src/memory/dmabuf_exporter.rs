use std::{fs::File, path::Path};

use crate::{
    device::Device,
    ioctl::{self, ExpbufFlags},
    Format, QueueType,
};

use anyhow::Result;

use super::MemoryType;

pub fn export_dmabufs(
    device_path: &Path,
    queue: QueueType,
    format: &Format,
    nb_buffers: usize,
) -> Result<Vec<File>> {
    let mut device = Device::open(device_path, Default::default())?;

    // TODO: check that the requested format has been set.
    let _set_format: Format = ioctl::s_fmt(&mut device, queue, format.clone()).unwrap();
    let nb_buffers: usize =
        ioctl::reqbufs(&device, queue, MemoryType::MMAP, nb_buffers as u32).unwrap();

    let fds: Vec<File> = (0..nb_buffers)
        .into_iter()
        .map(|i| ioctl::expbuf::<Device, File>(&device, queue, i, 0, ExpbufFlags::RDWR).unwrap())
        .collect();

    // We can close the device now, the exported buffers will remain alive as
    // long as they are referenced.
    drop(device);

    Ok(fds)
}

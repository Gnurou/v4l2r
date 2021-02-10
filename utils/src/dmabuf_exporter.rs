use std::{fs::File, path::Path};

use v4l2::{
    device::Device,
    ioctl::{self, ExpbufFlags},
    memory::{DMABufHandle, MemoryType},
    Format, QueueType,
};

use anyhow::Result;

pub fn export_dmabufs(
    device_path: &Path,
    queue: QueueType,
    format: &Format,
    nb_buffers: usize,
) -> Result<Vec<Vec<DMABufHandle<File>>>> {
    let mut device = Device::open(device_path, Default::default())?;

    // TODO: check that the requested format has been set.
    let _set_format: Format = ioctl::s_fmt(&mut device, queue, format.clone()).unwrap();
    let nb_buffers: usize =
        ioctl::reqbufs(&device, queue, MemoryType::MMAP, nb_buffers as u32).unwrap();

    let fds: Vec<Vec<DMABufHandle<File>>> = (0..nb_buffers)
        .into_iter()
        .map(|i| {
            vec![DMABufHandle::from(
                ioctl::expbuf::<Device, File>(&device, queue, i, 0, ExpbufFlags::RDWR).unwrap(),
            )]
        })
        .collect();

    // We can close the device now, the exported buffers will remain alive as
    // long as they are referenced.
    drop(device);

    Ok(fds)
}

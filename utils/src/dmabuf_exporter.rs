use std::{fs::File, path::Path};

use log::error;
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

    let set_format: Format = ioctl::s_fmt(&mut device, queue, format.clone()).unwrap();
    if set_format != *format {
        error!("Requested format does not apply as-is");
        error!("Requested format: {:?}", format);
        error!("Applied format: {:?}", format);
        return Err(anyhow::anyhow!("Could not apply requested format"));
    }
    let nb_buffers: usize =
        ioctl::reqbufs(&device, queue, MemoryType::MMAP, nb_buffers as u32).unwrap();

    let fds: Vec<Vec<DMABufHandle<File>>> = (0..nb_buffers)
        .into_iter()
        .map(|buffer| {
            (0..format.plane_fmt.len())
                .into_iter()
                .map(|plane| {
                    DMABufHandle::from(
                        ioctl::expbuf::<Device, File>(
                            &device,
                            queue,
                            buffer,
                            plane,
                            ExpbufFlags::RDWR,
                        )
                        .unwrap(),
                    )
                })
                .collect()
        })
        .collect();

    // We can close the device now, the exported buffers will remain alive as
    // long as they are referenced.
    drop(device);

    Ok(fds)
}

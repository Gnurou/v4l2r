use std::{fs::File, path::Path, sync::Arc};

use v4l2::{
    device::{queue::Queue, AllocatedQueue, Device},
    ioctl::{self, ExpbufFlags},
    memory::MMAPHandle,
    Format, QueueType,
};

use anyhow::Result;

pub fn export_dmabufs(
    device_path: &Path,
    queue: QueueType,
    format: &Format,
    nb_buffers: usize,
) -> Result<Vec<File>> {
    let device = Arc::new(Device::open(device_path, Default::default())?);
    let mut output_queue = Queue::get_output_mplane_queue(device.clone())?;

    // TODO: check which queue supports the requested format.
    output_queue.set_format(format.clone())?;

    let output_queue = output_queue.request_buffers::<Vec<MMAPHandle>>(nb_buffers as u32)?;

    let fds: Vec<File> = (0..output_queue.num_buffers())
        .into_iter()
        .map(|i| ioctl::expbuf::<Device, File>(&*device, queue, i, 0, ExpbufFlags::RDWR).unwrap())
        .collect();

    // We can close the device now, the exported buffers will remain alive as
    // long as they are referenced.
    drop(output_queue);
    drop(device);

    Ok(fds)
}

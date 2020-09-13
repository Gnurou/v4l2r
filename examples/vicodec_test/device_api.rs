use super::framegen;
use std::io::{self, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use qbuf::{get_free::GetFreeBuffer, Plane};
use v4l2::device::queue::*;
use v4l2::device::*;
use v4l2::memory::{UserPtr, MMAP};

/// Run a sample encoder on device `device_path`, which must be a `vicodec`
/// encoder instance. `lets_quit` will turn to true when Ctrl+C is pressed.
pub fn run<F: FnMut(&[u8])>(
    device_path: &Path,
    lets_quit: Arc<AtomicBool>,
    stop_after: Option<usize>,
    mut save_output: F,
) {
    let device = Device::open(device_path, DeviceConfig::new()).expect("Failed to open device");
    let caps = &device.capability;
    println!(
        "Opened device: {}\n\tdriver: {}\n\tbus: {}\n\tcapabilities: {}",
        caps.card, caps.driver, caps.bus_info, caps.capabilities
    );
    if caps.driver != "vicodec" {
        panic!(
            "This device is {}, but this test is designed to work with the vicodec driver.",
            caps.driver
        );
    }

    let device = Arc::new(device);

    // Obtain the queues, depending on whether we are using the single or multi planar API.
    let (mut output_queue, mut capture_queue, use_multi_planar) = if let Ok(output_queue) =
        Queue::get_output_queue(Arc::clone(&device))
    {
        (
            output_queue,
            Queue::get_capture_queue(Arc::clone(&device)).expect("Failed to obtain capture queue"),
            false,
        )
    } else if let Ok(output_queue) = Queue::get_output_mplane_queue(Arc::clone(&device)) {
        (
            output_queue,
            Queue::get_capture_mplane_queue(Arc::clone(&device))
                .expect("Failed to obtain capture queue"),
            true,
        )
    } else {
        panic!("Both single-planar and multi-planar queues are unusable.");
    };

    println!(
        "Multi-planar: {}",
        if use_multi_planar { "yes" } else { "no" }
    );

    println!("Output capabilities: {:?}", output_queue.get_capabilities());
    println!(
        "Capture capabilities: {:?}",
        capture_queue.get_capabilities()
    );

    println!("Output formats:");
    for fmtdesc in output_queue.format_iter() {
        println!("\t{}", fmtdesc);
    }

    println!("Capture formats:");
    for fmtdesc in capture_queue.format_iter() {
        println!("\t{}", fmtdesc);
    }

    // Make sure the CAPTURE queue will produce FWHT.
    let capture_format = capture_queue
        .change_format()
        .expect("Failed to get capture format")
        .set_pixelformat(b"FWHT")
        .apply()
        .expect("Failed to set capture format");

    if capture_format.pixelformat != b"FWHT".into() {
        panic!("FWHT format not supported on CAPTURE queue.");
    }

    // Set 640x480 RGB3 format on the OUTPUT queue.
    let output_format = output_queue
        .change_format()
        .expect("Failed to get output format")
        .set_size(640, 480)
        .set_pixelformat(b"RGB3")
        .apply()
        .expect("Failed to set output format");

    if output_format.pixelformat != b"RGB3".into() {
        panic!("RGB3 format not supported on OUTPUT queue.");
    }

    println!("Adjusted output format: {:?}", output_format);
    println!(
        "Adjusted capture format: {:?}",
        capture_queue
            .get_format()
            .expect("Failed to get capture format")
    );

    let output_image_size = output_format.plane_fmt[0].sizeimage as usize;
    let output_image_bytesperline = output_format.plane_fmt[0].bytesperline as usize;

    // Move the queues into their "allocated" state.
    let output_queue = output_queue
        .request_buffers::<UserPtr<Vec<u8>>>(2)
        .expect("Failed to allocate output buffers");
    let capture_queue = capture_queue
        .request_buffers::<MMAP>(2)
        .expect("Failed to allocate output buffers");
    println!(
        "Using {} output and {} capture buffers.",
        output_queue.num_buffers(),
        capture_queue.num_buffers()
    );

    // Create backing memory for the OUTPUT buffers.
    let mut output_frame = Some(vec![0u8; output_image_size]);

    output_queue
        .stream_on()
        .expect("Failed to start output_queue");
    capture_queue.stream_on().expect("Failed to start capture");

    let mut cpt = 0usize;
    let mut total_size = 0usize;
    let start_time = Instant::now();
    // Encode generated frames until Ctrl+c is pressed.
    while !lets_quit.load(Ordering::SeqCst) {
        if let Some(max_cpt) = stop_after {
            if cpt >= max_cpt {
                break;
            }
        }

        let mut output_buffer_data = output_frame
            .take()
            .expect("Output buffer not available. This is a bug.");

        framegen::gen_pattern(
            &mut output_buffer_data[..],
            output_image_bytesperline,
            cpt as u32,
        );

        // There is no information to set on MMAP capture buffers: just queue
        // them as soon as we get them.
        capture_queue
            .try_get_free_buffer()
            .expect("Failed to obtain capture buffer")
            .auto_queue()
            .expect("Failed to queue capture buffer");

        // USERPTR output buffers, on the other hand, must be set up with
        // a user buffer and bytes_used.
        // The queue takes ownership of the buffer until the driver is done
        // with it.
        let bytes_used = output_buffer_data.len();
        output_queue
            .try_get_free_buffer()
            .expect("Failed to obtain output buffer")
            .add_plane(Plane::out_with_handle(output_buffer_data, bytes_used))
            .queue()
            .expect("Failed to queue output buffer");

        // Now dequeue the work that we just scheduled.

        let mut out_dqbuf = output_queue
            .try_dequeue()
            .expect("Failed to dequeue output buffer");

        // Make the buffer data available again. It should have been empty since
        // the buffer was owned by the queue.
        assert_eq!(
            output_frame.replace(out_dqbuf.plane_handles.remove(0)),
            None
        );

        let cap_dqbuf = capture_queue
            .try_dequeue()
            .expect("Failed to dequeue capture buffer");
        let cap_index = cap_dqbuf.data.index as usize;
        let bytes_used = cap_dqbuf.data.planes[0].bytesused as usize;

        total_size = total_size.wrapping_add(bytes_used);
        let elapsed = start_time.elapsed();
        let fps = cpt as f64 / elapsed.as_millis() as f64 * 1000.0;
        print!(
            "\rEncoded buffer {:#5}, {:#2} -> {:#2}), bytes used:{:#6} total encoded size:{:#8} fps: {:#5.2}",
            cap_dqbuf.data.sequence,
            out_dqbuf.data.index,
            cap_index,
            bytes_used,
            total_size,
            fps
        );
        io::stdout().flush().unwrap();

        let cap_mapping = cap_dqbuf
            .get_plane_mapping(0)
            .expect("Failed to map capture buffer");
        save_output(cap_mapping.as_ref());

        cpt = cpt.wrapping_add(1);
    }

    capture_queue
        .stream_off()
        .expect("Failed to stop output_queue");
    output_queue
        .stream_off()
        .expect("Failed to stop output_queue");
}

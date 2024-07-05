use nix::fcntl::{open, OFlag};
use nix::sys::stat::Mode;
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{self, Write};
use std::os::unix::io::FromRawFd;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;
use v4l2r_utils::framegen::FrameGenerator;

use v4l2r::memory::{MemoryType, MmapHandle};
use v4l2r::{ioctl::*, memory::UserPtrHandle};
use v4l2r::{Format, QueueType::*};

/// Run a sample encoder on device `device_path`, which must be a `vicodec`
/// encoder instance. `lets_quit` will turn to true when Ctrl+C is pressed.
pub fn run<F: FnMut(&[u8])>(
    device_path: &Path,
    output_mem: MemoryType,
    capture_mem: MemoryType,
    lets_quit: Arc<AtomicBool>,
    stop_after: Option<usize>,
    mut save_output: F,
) {
    let mut fd = unsafe {
        File::from_raw_fd(
            open(device_path, OFlag::O_RDWR | OFlag::O_CLOEXEC, Mode::empty())
                .unwrap_or_else(|_| panic!("Cannot open {}", device_path.display())),
        )
    };

    // Check that we are dealing with vicodec.
    let caps: Capability = querycap(&fd).expect("Failed to get device capacities");
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

    // Check whether the driver uses the single or multi-planar API by
    // requesting 0 MMAP buffers on the OUTPUT queue. The working queue will
    // return a success.
    let (output_queue_type, _capture_queue_type, use_multi_planar) =
        if reqbufs::<()>(&fd, VideoOutput, MemoryType::Mmap, 0).is_ok() {
            (VideoOutput, VideoCapture, false)
        } else if reqbufs::<()>(&fd, VideoOutputMplane, MemoryType::Mmap, 0).is_ok() {
            (VideoOutputMplane, VideoCaptureMplane, true)
        } else {
            panic!("Both single-planar and multi-planar queues are unusable.");
        };
    println!(
        "Multi-planar: {}",
        if use_multi_planar { "yes" } else { "no" }
    );

    let (output_queue, capture_queue) = match use_multi_planar {
        false => (VideoOutput, VideoCapture),
        true => (VideoOutputMplane, VideoCaptureMplane),
    };

    // List the output formats.
    let out_formats = FormatIterator::new(&fd, output_queue)
        .map(|f| (f.pixelformat, f))
        .collect::<BTreeMap<_, _>>();
    println!("Output formats:");
    for (_, fmtdesc) in out_formats.iter() {
        println!("\t{}", fmtdesc);
    }

    // List the capture formats.
    let cap_formats = FormatIterator::new(&fd, capture_queue)
        .map(|f| (f.pixelformat, f))
        .collect::<BTreeMap<_, _>>();
    println!("Capture formats:");
    for (_, fmtdesc) in cap_formats.iter() {
        println!("\t{}", fmtdesc);
    }

    // We will encode from RGB3 to FWHT.
    if !out_formats.contains_key(&b"RGB3".into()) {
        panic!("RGB3 format not supported on OUTPUT queue.");
    }

    if !cap_formats.contains_key(&b"FWHT".into()) {
        panic!("FWHT format not supported on CAPTURE queue.");
    }

    let mut capture_format: Format =
        g_fmt(&fd, capture_queue).expect("Failed getting capture format");
    // Let's just make sure the encoding format on the CAPTURE queue is FWHT.
    capture_format.pixelformat = b"FWHT".into();
    println!("Setting capture format: {:?}", capture_format);
    let _capture_format: Format =
        s_fmt(&mut fd, (capture_queue, &capture_format)).expect("Failed setting capture format");

    // We will be happy with 640x480 resolution.
    let output_format = Format {
        width: 640,
        height: 480,
        pixelformat: b"RGB3".into(),
        ..Default::default()
    };

    println!("Setting output format: {:?}", output_format);
    let output_format: Format =
        s_fmt(&mut fd, (output_queue, &output_format)).expect("Failed setting output format");

    let capture_format: Format = g_fmt(&fd, capture_queue).expect("Failed getting capture format");
    println!("Adjusted output format: {:?}", output_format);
    println!("Adjusted capture format: {:?}", capture_format);

    match output_mem {
        MemoryType::Mmap => (),
        MemoryType::UserPtr => (),
        m => panic!("Unsupported OUTPUT memory type {:?}", m),
    }

    match capture_mem {
        MemoryType::Mmap => (),
        m => panic!("Unsupported CAPTURE memory type {:?}", m),
    }

    // We could run this with as little as one buffer, but let's cycle between
    // two for the sake of it.
    // For simplicity the OUTPUT buffers will use user memory.
    let num_output_buffers: usize =
        reqbufs(&fd, output_queue, output_mem, 2).expect("Failed to allocate output buffers");
    let num_capture_buffers: usize =
        reqbufs(&fd, capture_queue, capture_mem, 2).expect("Failed to allocate capture buffers");
    println!(
        "Using {} output and {} capture buffers.",
        num_output_buffers, num_capture_buffers
    );

    let mut capture_mappings = Vec::new();
    for i in 0..num_capture_buffers {
        let query_buf: QueryBuffer =
            querybuf(&fd, capture_queue, i).expect("Failed to query buffer");
        println!(
            "Capture buffer {} at offset 0x{:0x}, length 0x{:0x}",
            i, query_buf.planes[0].mem_offset, query_buf.planes[0].length
        );
        capture_mappings.push(
            mmap(
                &fd,
                query_buf.planes[0].mem_offset,
                query_buf.planes[0].length,
            )
            .expect("Failed to map buffer"),
        );
    }

    let output_image_size = output_format.plane_fmt[0].sizeimage as usize;
    let mut output_buffers: Vec<UserPtrHandle<Vec<u8>>> = match output_mem {
        MemoryType::Mmap => Default::default(),
        MemoryType::UserPtr => std::iter::repeat(vec![0u8; output_image_size])
            .take(num_output_buffers)
            .map(UserPtrHandle::from)
            .collect(),
        _ => unreachable!(),
    };

    // Start streaming.
    streamon(&fd, output_queue).expect("Failed to start output queue");
    streamon(&fd, capture_queue).expect("Failed to start capture queue");

    let mut frame_gen = FrameGenerator::new(
        output_format.width as usize,
        output_format.height as usize,
        output_format.plane_fmt[0].bytesperline as usize,
    )
    .expect("Failed to create frame generator");

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

        let output_buffer_index = cpt % num_output_buffers;
        let capture_buffer_index = cpt % num_output_buffers;

        // Generate the frame data and buffer to queue.
        match output_mem {
            MemoryType::Mmap => {
                let buffer_info: QueryBuffer =
                    querybuf(&fd, output_queue_type, output_buffer_index)
                        .expect("Failed to query output buffer");
                let plane = &buffer_info.planes[0];
                let mut mapping =
                    mmap(&fd, plane.mem_offset, plane.length).expect("Failed to map output buffer");

                frame_gen
                    .next_frame(&mut mapping)
                    .expect("Failed to generate frame");

                let mut out_qbuf =
                    QBuffer::<MmapHandle>::new(output_queue, output_buffer_index as u32);
                out_qbuf.planes = vec![QBufPlane::new(frame_gen.frame_size())];

                qbuf::<_, ()>(&fd, out_qbuf)
            }
            MemoryType::UserPtr => {
                let output_buffer = &mut output_buffers[output_buffer_index];

                frame_gen
                    .next_frame(&mut output_buffer.0)
                    .expect("Failed to generate frame");

                let mut out_qbuf = QBuffer::<UserPtrHandle<Vec<u8>>>::new(
                    output_queue,
                    output_buffer_index as u32,
                );
                out_qbuf.planes = vec![QBufPlane::new_from_handle(
                    output_buffer,
                    output_buffer.0.len(),
                )];

                qbuf::<_, ()>(&fd, out_qbuf)
            }
            _ => unreachable!(),
        }
        .expect("Error queueing output buffer");

        let mut cap_qbuf = QBuffer::<MmapHandle>::new(capture_queue, capture_buffer_index as u32);
        cap_qbuf.planes = vec![QBufPlane::new(0)];

        qbuf::<_, ()>(&fd, cap_qbuf).expect("Error queueing capture buffer");

        // Now dequeue the work that we just scheduled.

        // We can disregard the OUTPUT buffer since it does not contain any
        // useful data for us.
        dqbuf::<()>(&fd, output_queue).expect("Failed to dequeue output buffer");

        // The CAPTURE buffer, on the other hand, we want to examine more closely.
        let cap_dqbuf: V4l2Buffer =
            dqbuf(&fd, capture_queue).expect("Failed to dequeue capture buffer");
        let bytes_used = *cap_dqbuf.get_first_plane().bytesused as usize;

        total_size = total_size.wrapping_add(bytes_used);
        let elapsed = start_time.elapsed();
        let fps = cpt as f64 / elapsed.as_millis() as f64 * 1000.0;
        print!(
            "\rEncoded buffer {:#5}, index: {:#2}), bytes used:{:#6} total encoded size:{:#8} fps: {:#5.2}",
            cap_dqbuf.sequence(), cap_dqbuf.index(), bytes_used, total_size, fps
        );
        io::stdout().flush().unwrap();

        save_output(&capture_mappings[cap_dqbuf.index() as usize].as_ref()[0..bytes_used]);

        cpt = cpt.wrapping_add(1);
    }

    // Stop streaming.
    streamoff(&fd, capture_queue).expect("Failed to stop capture queue");
    streamoff(&fd, output_queue).expect("Failed to stop output queue");

    // Clear the mappings
    drop(capture_mappings);

    // Free the buffers.
    reqbufs::<()>(&fd, capture_queue, MemoryType::Mmap, 0)
        .expect("Failed to release capture buffers");
    reqbufs::<()>(&fd, output_queue, MemoryType::UserPtr, 0)
        .expect("Failed to release output buffers");

    // The fd will be closed as the File instance gets out of scope.
}

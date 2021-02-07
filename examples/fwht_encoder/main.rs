mod framegen;

use framegen::FrameGenerator;

use std::fs::File;
use std::io::{self, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::{cell::RefCell, collections::VecDeque, time::Instant};

use v4l2::{
    device::queue::{
        direction::Capture,
        dqbuf::DQBuffer,
        generic::{GenericBufferHandles, GenericQBuffer, GenericSupportedMemoryType},
        qbuf::OutputQueueable,
    },
    encoder::*,
    memory::{dmabuf_exporter, MMAPHandle, MMAPProvider, UserPtrHandle},
    Format, QueueType,
};

use anyhow::ensure;
use clap::{App, Arg};

const FRAME_SIZE: (usize, usize) = (640, 480);

fn main() {
    let matches = App::new("FWHT encoder")
        .arg(
            Arg::with_name("num_frames")
                .long("stop_after")
                .takes_value(true)
                .help("Stop after encoding a given number of buffers"),
        )
        .arg(
            Arg::with_name("device")
                .required(true)
                .help("Path to the vicodec device file"),
        )
        .arg(
            Arg::with_name("output_file")
                .long("save")
                .required(false)
                .takes_value(true)
                .help("Save the encoded stream to a file"),
        )
        .arg(
            Arg::with_name("output_mem")
                .long("output_mem")
                .required(false)
                .takes_value(true)
                .default_value("user")
                .help("Type of output memory to use (mmap or user)"),
        )
        .get_matches();

    let device_path = matches.value_of("device").unwrap_or("/dev/video0");

    let mut stop_after = match clap::value_t!(matches.value_of("num_frames"), usize) {
        Ok(v) => Some(v),
        Err(e) if e.kind == clap::ErrorKind::ArgumentNotFound => None,
        Err(e) => panic!("Invalid value for stop_after: {}", e),
    };

    let mut output_file = matches
        .value_of("output_file")
        .map(|s| File::create(s).expect("Invalid output file specified."));

    let output_mem = match matches.value_of("output_mem") {
        Some("mmap") => GenericSupportedMemoryType::MMAP,
        Some("user") => GenericSupportedMemoryType::UserPtr,
        Some("dmabuf") => GenericSupportedMemoryType::DMABuf,
        _ => panic!("Invalid value for output_mem"),
    };

    let lets_quit = Arc::new(AtomicBool::new(false));
    // Setup the Ctrl+c handler.
    {
        let lets_quit_handler = lets_quit.clone();
        ctrlc::set_handler(move || {
            lets_quit_handler.store(true, Ordering::SeqCst);
        })
        .expect("Failed to set Ctrl-C handler.");
    }

    let encoder = Encoder::open(&Path::new(&device_path))
        .expect("Failed to open device")
        .set_capture_format(|f| {
            let format: Format = f.set_pixelformat(b"FWHT").apply()?;

            ensure!(
                format.pixelformat == b"FWHT".into(),
                "FWHT format not supported"
            );

            Ok(())
        })
        .expect("Failed to set capture format")
        .set_output_format(|f| {
            let format: Format = f
                .set_pixelformat(b"RGB3")
                .set_size(FRAME_SIZE.0, FRAME_SIZE.1)
                .apply()?;

            ensure!(
                format.pixelformat == b"RGB3".into(),
                "RGB3 format not supported"
            );
            ensure!(
                format.width as usize == FRAME_SIZE.0 && format.height as usize == FRAME_SIZE.1,
                "Output frame resolution not supported"
            );

            Ok(())
        })
        .expect("Failed to set output format");

    let output_format = encoder
        .get_output_format()
        .expect("Failed to get output format");
    println!("Adjusted output format: {:?}", output_format);

    let dmabuf_fds = dmabuf_exporter::export_dmabufs(
        &Path::new(&device_path),
        QueueType::VideoOutputMplane,
        &output_format,
        NUM_BUFFERS,
    )
    .unwrap();
    println!("DMABufs: {:?}", dmabuf_fds);
    let dmabufs: RefCell<VecDeque<_>> = RefCell::new(dmabuf_fds.into_iter().collect());

    let capture_format = encoder
        .get_capture_format()
        .expect("Failed to get capture format");
    println!("Adjusted capture format: {:?}", capture_format);

    println!(
        "Configured encoder for {}x{} ({} bytes per line)",
        output_format.width, output_format.height, output_format.plane_fmt[0].bytesperline
    );

    let mut frame_gen = FrameGenerator::new(
        output_format.width as usize,
        output_format.height as usize,
        output_format.plane_fmt[0].bytesperline as usize,
    )
    .expect("Failed to create frame generator");

    const NUM_BUFFERS: usize = 2;

    let free_buffers: Option<VecDeque<_>> = match output_mem {
        GenericSupportedMemoryType::MMAP | GenericSupportedMemoryType::DMABuf => None,
        GenericSupportedMemoryType::UserPtr => Some(
            std::iter::repeat(vec![0u8; output_format.plane_fmt[0].sizeimage as usize])
                .take(NUM_BUFFERS)
                .collect(),
        ),
    };
    let free_buffers = RefCell::new(free_buffers);

    let input_done_cb = |buffer: CompletedOutputBuffer<GenericBufferHandles>| {
        let handles = match buffer {
            CompletedOutputBuffer::Dequeued(mut buf) => buf.take_handles().unwrap(),
            CompletedOutputBuffer::Canceled(buf) => buf.plane_handles,
        };
        match handles {
            // We have nothing to do for MMAP buffers.
            GenericBufferHandles::MMAP(_) => {}
            // For user-allocated memory, return the buffer to the free list.
            GenericBufferHandles::User(mut u) => {
                free_buffers
                    .borrow_mut()
                    .as_mut()
                    .unwrap()
                    .push_back(u.remove(0).0);
            }
            GenericBufferHandles::DMABuf(d) => {
                dmabufs.borrow_mut().push_back(d);
            }
        };
    };

    let mut total_size = 0usize;
    let start_time = Instant::now();
    let poll_count_reader = Arc::new(AtomicUsize::new(0));
    let poll_count_writer = Arc::clone(&poll_count_reader);
    let output_ready_cb = move |cap_dqbuf: DQBuffer<Capture, Vec<MMAPHandle>>| {
        let bytes_used = cap_dqbuf.data.get_first_plane().bytesused() as usize;
        total_size = total_size.wrapping_add(bytes_used);
        let elapsed = start_time.elapsed();
        let frame_nb = cap_dqbuf.data.sequence() + 1;
        let fps = frame_nb as f32 / elapsed.as_millis() as f32 * 1000.0;
        let ppf = poll_count_reader.load(Ordering::SeqCst) as f32 / frame_nb as f32;
        print!(
            "\rEncoded buffer {:#5}, index: {:#2}), bytes used:{:#6} total encoded size:{:#8} fps: {:#5.2} ppf: {:#4.2}" ,
            cap_dqbuf.data.sequence(),
            cap_dqbuf.data.index(),
            bytes_used,
            total_size,
            fps,
            ppf,
        );
        io::stdout().flush().unwrap();

        if let Some(ref mut output) = output_file {
            let mapping = cap_dqbuf
                .get_plane_mapping(0)
                .expect("Failed to map capture buffer");
            output
                .write_all(mapping.as_ref())
                .expect("Error while writing output data");
        }
    };

    let mut encoder = encoder
        .allocate_output_buffers_generic::<GenericBufferHandles>(output_mem, NUM_BUFFERS)
        .expect("Failed to allocate OUTPUT buffers")
        .allocate_capture_buffers(NUM_BUFFERS, MMAPProvider::new(&capture_format))
        .expect("Failed to allocate CAPTURE buffers")
        .set_poll_counter(poll_count_writer)
        .start(input_done_cb, output_ready_cb)
        .expect("Failed to start encoder");

    while !lets_quit.load(Ordering::SeqCst) {
        if let Some(max_cpt) = &mut stop_after {
            if *max_cpt == 0 {
                break;
            }
            *max_cpt -= 1;
        }

        let v4l2_buffer = match encoder.get_buffer() {
            Ok(buffer) => buffer,
            // If we got interrupted while waiting for a buffer, just exit normally.
            Err(GetBufferError::PollError(e)) if e.kind() == io::ErrorKind::Interrupted => break,
            Err(e) => panic!(e),
        };
        let bytes_used = frame_gen.frame_size();
        match v4l2_buffer {
            GenericQBuffer::MMAP(buf) => {
                let mut mapping = buf
                    .get_plane_mapping(0)
                    .expect("Failed to get MMAP mapping");
                frame_gen
                    .next_frame(&mut mapping)
                    .expect("Failed to generate frame");
                buf.queue(&[bytes_used])
                    .expect("Failed to queue input frame");
            }
            GenericQBuffer::User(buf) => {
                let mut buffer = free_buffers
                    .borrow_mut()
                    .as_mut()
                    .unwrap()
                    .pop_front()
                    .expect("No backing buffer to bind");
                frame_gen
                    .next_frame(&mut buffer)
                    .expect("Failed to generate frame");
                buf.queue_with_handles(
                    GenericBufferHandles::from(vec![UserPtrHandle::from(buffer)]),
                    &[bytes_used],
                )
                .expect("Failed to queue input frame");
            }
            GenericQBuffer::DMABuf(buf) => {
                let buffer = dmabufs
                    .borrow_mut()
                    .pop_front()
                    .expect("No backing dmabuf to bind");
                let mut mapping = buffer[0].map().unwrap();
                frame_gen
                    .next_frame(&mut mapping)
                    .expect("Failed to generate frame");
                buf.queue_with_handles(GenericBufferHandles::from(buffer), &[bytes_used])
                    .expect("Failed to queue input frame");
            }
        }
    }

    encoder.stop().unwrap();

    // Insert new line since we were overwriting the same one
    println!();

    if output_mem == GenericSupportedMemoryType::UserPtr {
        // All the OUTPUT buffers should have been returned
        assert_eq!(free_buffers.borrow().as_ref().unwrap().len(), NUM_BUFFERS);
    }
}

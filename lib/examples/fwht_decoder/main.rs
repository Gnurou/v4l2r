use std::{
    fs::File,
    io,
    io::BufReader,
    io::Write,
    path::Path,
    sync::atomic::AtomicBool,
    sync::atomic::{AtomicUsize, Ordering},
    sync::Arc,
};

use anyhow::ensure;
use v4l2r::decoder::stateful::GetBufferError;
use v4l2r::{
    decoder::{format::fwht::FwhtFrameParser, FormatChangedReply},
    device::queue::{
        handles_provider::{PooledHandles, PooledHandlesProvider},
        qbuf::OutputQueueable,
        FormatBuilder,
    },
    memory::{MemoryType, UserPtrHandle},
};
use v4l2r::{
    decoder::{stateful::Decoder, DecoderEvent},
    device::{
        poller::PollError,
        queue::{direction::Capture, dqbuf::DqBuffer},
    },
    memory::{DmaBufHandle, DmaBufferHandles},
    Format, Rect,
};

use clap::{App, Arg};

fn main() {
    env_logger::init();

    let matches = App::new("FWHT decoder")
        .arg(
            Arg::with_name("stream")
                .required(true)
                .help("Path to the FWHT stream to decode"),
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
                .help("Save the decoded RGB frames to a file"),
        )
        .get_matches();

    let stream_path = matches
        .value_of("stream")
        .expect("Stream argument not specified");
    let device_path = matches
        .value_of("device")
        .expect("Device argument not specified");

    let stream = BufReader::new(File::open(stream_path).expect("Compressed stream not found"));

    let mut output_file: Option<File> = matches
        .value_of("output_file")
        .map(|path| File::create(path).expect("Invalid output file specified."));

    let lets_quit = Arc::new(AtomicBool::new(false));
    // Setup the Ctrl+c handler.
    {
        let lets_quit_handler = lets_quit.clone();
        ctrlc::set_handler(move || {
            lets_quit_handler.store(true, Ordering::SeqCst);
        })
        .expect("Failed to set Ctrl-C handler.");
    }

    const NUM_OUTPUT_BUFFERS: usize = 4;

    let poll_count_reader = Arc::new(AtomicUsize::new(0));
    let poll_count_writer = Arc::clone(&poll_count_reader);
    let start_time = std::time::Instant::now();
    let mut output_buffer_size = 0usize;
    let mut frame_counter = 0usize;
    let mut output_ready_cb =
        move |mut cap_dqbuf: DqBuffer<Capture, PooledHandles<DmaBufferHandles<File>>>| {
            let bytes_used = cap_dqbuf.data.get_first_plane().bytesused() as usize;
            // Ignore zero-sized buffers.
            if bytes_used == 0 {
                return;
            }

            let elapsed = start_time.elapsed();
            frame_counter += 1;
            let fps = frame_counter as f32 / elapsed.as_millis() as f32 * 1000.0;
            let ppf = poll_count_reader.load(Ordering::SeqCst) as f32 / frame_counter as f32;
            print!(
                "\rDecoded buffer {:#5}, index: {:#2}), bytes used:{:#6} fps: {:#5.2} ppf: {:#4.2}",
                cap_dqbuf.data.sequence(),
                cap_dqbuf.data.index(),
                bytes_used,
                fps,
                ppf,
            );
            io::stdout().flush().unwrap();

            if let Some(ref mut output) = output_file {
                let pooled_handles = cap_dqbuf.take_handles().unwrap();
                let handles = pooled_handles.handles();
                let mapping = handles[0].map().expect("Failed to map capture buffer");
                output
                    .write_all(&mapping)
                    .expect("Error while writing output data");
            }
        };
    let decoder_event_cb =
        move |event: DecoderEvent<PooledHandlesProvider<Vec<DmaBufHandle<File>>>>| match event {
            DecoderEvent::FrameDecoded(dqbuf) => output_ready_cb(dqbuf),
            DecoderEvent::EndOfStream => (),
        };
    type PooledDmaBufHandlesProvider = PooledHandlesProvider<Vec<DmaBufHandle<File>>>;
    let set_capture_format_cb =
        move |f: FormatBuilder,
              visible_rect: Rect,
              min_num_buffers: usize|
              -> anyhow::Result<FormatChangedReply<PooledDmaBufHandlesProvider>> {
            // Let's keep the pixel format that the decoder found convenient.
            let format = f.format();

            println!(
                "New CAPTURE format: {:?} (visible rect: {})",
                format, visible_rect
            );

            let dmabuf_fds: Vec<Vec<_>> =
                utils::dmabuf_exporter::export_dmabufs(&format, min_num_buffers).unwrap();

            Ok(FormatChangedReply {
                provider: PooledHandlesProvider::new(dmabuf_fds),
                // TODO: can't the provider report the memory type that it is
                // actually serving itself?
                mem_type: MemoryType::DmaBuf,
                num_buffers: min_num_buffers,
            })
        };

    let mut decoder = Decoder::open(&Path::new(&device_path))
        .expect("Failed to open device")
        .set_output_format(|f| {
            let format: Format = f.set_pixelformat(b"FWHT").apply()?;

            ensure!(
                format.pixelformat == b"FWHT".into(),
                "FWHT format not supported"
            );

            println!("Tentative OUTPUT format: {:?}", format);

            output_buffer_size = format.plane_fmt[0].sizeimage as usize;

            Ok(())
        })
        .expect("Failed to set output format")
        .allocate_output_buffers::<Vec<UserPtrHandle<Vec<u8>>>>(NUM_OUTPUT_BUFFERS)
        .expect("Failed to allocate output buffers")
        .set_poll_counter(poll_count_writer)
        .start(|_| (), decoder_event_cb, set_capture_format_cb)
        .expect("Failed to start decoder");

    // Remove mutability.
    let output_buffer_size = output_buffer_size;

    println!("Allocated {} buffers", decoder.num_output_buffers());
    println!("Required size for output buffers: {}", output_buffer_size);

    let parser = FwhtFrameParser::new(stream)
        .unwrap_or_else(|| panic!("No FWHT stream detected in {}", stream_path));

    'mainloop: for (_cpt, mut frame) in parser.enumerate() {
        // Ctrl-c ?
        if lets_quit.load(Ordering::SeqCst) {
            break;
        }

        let bytes_used = frame.len();

        // Resize to the minimum required OUTPUT buffer size.
        if frame.len() < output_buffer_size {
            frame.resize(output_buffer_size, 0u8);
        }

        let v4l2_buffer = match decoder.get_buffer() {
            Ok(buffer) => buffer,
            // If we got interrupted while waiting for a buffer, just exit normally.
            Err(GetBufferError::PollError(PollError::EPollWait(e)))
                if e.kind() == io::ErrorKind::Interrupted =>
            {
                break 'mainloop
            }
            Err(e) => panic!("{}", e),
        };

        v4l2_buffer
            .queue_with_handles(vec![UserPtrHandle::from(frame)], &[bytes_used])
            .expect("Failed to queue input frame");
    }

    decoder.drain(true).unwrap();
    decoder.stop().unwrap();
    println!();
}

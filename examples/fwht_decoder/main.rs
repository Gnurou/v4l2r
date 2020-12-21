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
use v4l2::{
    decoder::format::fwht::FwhtFrameParser,
    device::queue::{qbuf::OutputQueueable, FormatBuilder},
    memory::UserPtrHandle,
};
use v4l2::{
    decoder::stateful::Decoder,
    device::queue::{direction::Capture, dqbuf::DQBuffer},
};
use v4l2::{decoder::stateful::GetBufferError, memory::MMAPHandle};

use clap::{App, Arg};

fn main() {
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

    let mut output_file: Option<File> = if let Some(path) = matches.value_of("output_file") {
        Some(File::create(path).expect("Invalid output file specified."))
    } else {
        None
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

    const NUM_OUTPUT_BUFFERS: usize = 4;

    let poll_count_reader = Arc::new(AtomicUsize::new(0));
    let poll_count_writer = Arc::clone(&poll_count_reader);
    let start_time = std::time::Instant::now();
    let mut output_buffer_size = 0usize;
    let output_ready_cb = move |cap_dqbuf: DQBuffer<Capture, Vec<MMAPHandle>>| {
        let bytes_used = cap_dqbuf.data.planes[0].bytesused as usize;
        let elapsed = start_time.elapsed();
        let frame_nb = cap_dqbuf.data.sequence + 1;
        let fps = frame_nb as f32 / elapsed.as_millis() as f32 * 1000.0;
        let ppf = poll_count_reader.load(Ordering::SeqCst) as f32 / frame_nb as f32;
        print!(
            "\rEncoded buffer {:#5}, index: {:#2}), bytes used:{:#6} fps: {:#5.2} ppf: {:#4.2}",
            cap_dqbuf.data.sequence, cap_dqbuf.data.index, bytes_used, fps, ppf,
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
    let set_capture_format_cb = |f: FormatBuilder| -> anyhow::Result<()> {
        let capture_format = f.set_pixelformat(b"RGB3").apply()?;

        println!("New CAPTURE format: {:?}", capture_format);
        Ok(())
    };

    let mut decoder = Decoder::open(&Path::new(&device_path))
        .expect("Failed to open device")
        .set_output_format(|f| {
            let format = f.set_pixelformat(b"FWHT").apply()?;

            ensure!(
                format.pixelformat == b"FWHT".into(),
                "FWHT format not supported"
            );

            println!("Temporary output format: {:?}", format);

            output_buffer_size = format.plane_fmt[0].sizeimage as usize;

            Ok(())
        })
        .expect("Failed to set output format")
        .allocate_output_buffers(NUM_OUTPUT_BUFFERS)
        .expect("Failed to allocate output buffers")
        .set_poll_counter(poll_count_writer)
        .start(|_| (), output_ready_cb, set_capture_format_cb)
        .expect("Failed to start decoder");
    // TODO we also need a way to provide the buffers to the CAPTURE queue.
    // Probably need to design a new buffer provider interface for that.
    // i.e. BackingMemoryProvider<Vec<u8>>
    // For now let's just use MMAP.

    // Remove mutability.
    let output_buffer_size = output_buffer_size;

    println!("Allocated {} buffers", decoder.num_output_buffers());
    println!("Required size for output buffers: {}", output_buffer_size);

    let parser = FwhtFrameParser::new(stream)
        .unwrap_or_else(|| panic!("No FWHT stream detected in {}", stream_path));

    let mut total_size: usize = 0;

    for (_cpt, mut frame) in parser.enumerate() {
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
            Err(GetBufferError::PollError(e)) if e.kind() == io::ErrorKind::Interrupted => break,
            Err(e) => panic!(e),
        };

        v4l2_buffer
            .queue_with_handles(vec![UserPtrHandle::from(frame)], &[bytes_used])
            .expect("Failed to queue input frame");

        total_size += bytes_used;
    }

    decoder.stop().unwrap();
    println!();

    println!("Total size: {}", total_size);
}

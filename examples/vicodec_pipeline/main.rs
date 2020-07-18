mod encoder;
mod framegen;

use encoder::client;
use encoder::*;
use framegen::FrameGenerator;

use ctrlc;
use std::io::{self, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use anyhow::ensure;

use clap::{App, Arg};

const FRAME_SIZE: (usize, usize) = (640, 480);

fn main() {
    let matches = App::new("vicodec example")
        .arg(
            Arg::with_name("stop_after")
                .long("stop_after")
                .takes_value(true)
                .help("Stop after encoding this number of buffers"),
        )
        .arg(
            Arg::with_name("device")
                .required(true)
                .help("Path to the vicodec device file"),
        )
        .get_matches();

    let device_path = matches.value_of("device").unwrap_or("/dev/video0");
    let mut stop_after = match clap::value_t!(matches.value_of("stop_after"), usize) {
        Ok(v) => Some(v),
        Err(e) if e.kind == clap::ErrorKind::ArgumentNotFound => None,
        Err(e) => panic!("Invalid value for stop_after: {}", e),
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

    let encoder = Encoder::open(&Path::new(&device_path)).unwrap();
    let (encoder, capture_format) = encoder
        .set_capture_format(|f| {
            let format = f
                .set_pixelformat(b"FWHT")
                .set_size(FRAME_SIZE.0, FRAME_SIZE.1)
                .apply()?;

            ensure!(
                format.pixelformat == b"FWHT".into(),
                "FWHT format not supported"
            );
            ensure!(
                format.width as usize == FRAME_SIZE.0 && format.height as usize == FRAME_SIZE.1,
                "Frame resolution not supported on capture queue"
            );

            Ok(format)
        })
        .unwrap();

    let (encoder, output_format) = encoder
        .set_output_format(|f| {
            let format = f
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

            Ok(format)
        })
        .unwrap();

    println!("Adjusted output format: {:?}", output_format);
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
    .unwrap();

    const NUM_BUFFERS: usize = 2;

    let encoder = encoder.allocate_buffers(NUM_BUFFERS, NUM_BUFFERS).unwrap();
    let client = encoder.start_encoding().unwrap();

    use std::collections::VecDeque;

    let mut free_buffers = VecDeque::new();
    for _ in 0..NUM_BUFFERS + 0 {
        free_buffers.push_back(vec![0u8; output_format.plane_fmt[0].sizeimage as usize]);
    }

    let mut total_size = 0usize;
    let mut next_buffer: Option<Vec<u8>> = None;
    let start_time = Instant::now();
    while !lets_quit.load(Ordering::SeqCst) {
        // Generate the next buffer to encode if there is none yet.
        if next_buffer.is_none() {
            if let Some(mut buffer) = free_buffers.pop_front() {
                frame_gen.next_frame(&mut buffer[..]).unwrap();
                next_buffer = Some(buffer);
            }
        }

        // Try to queue the next buffer and wait for a message from the encoder.
        let msg = if let Some(buffer) = next_buffer.take() {
            match client.encode(buffer) {
                // Queue succeeded, try to process one encoder message or keep
                // queueing new buffers if there is none.
                Ok(()) => None,
                // V4L2 queue is full, replace the buffer and wait for a message
                // from the encoder.
                Err(client::EncodeError::QueueFull(buffer)) => {
                    next_buffer = Some(buffer);
                    Some(client.recv.recv().unwrap())
                }
                _ => panic!("Fatal error"),
            }
        } else {
            Some(client.recv.recv().unwrap())
        };

        // TODO detect channel closing as a platform error.
        for msg in msg.into_iter().chain(client.recv.try_iter()) {
            match msg {
                Message::InputBufferDone(buffer) => free_buffers.push_back(buffer),
                Message::FrameEncoded(cap_dqbuf) => {
                    total_size =
                        total_size.wrapping_add(cap_dqbuf.data.planes[0].bytesused as usize);
                    let frame_nb = cap_dqbuf.data.sequence + 1;
                    let elapsed = start_time.elapsed();
                    let fps = frame_nb as f32 / elapsed.as_millis() as f32 * 1000.0;
                    let num_poll_wakeups = client.num_poll_wakeups.load(Ordering::SeqCst);
                    print!(
                        "\rEncoded buffer {:#5}, index: {:#2}), bytes used:{:#6} total encoded size:{:#8} fps: {:#5.2} ppf: {:#2.2}",
                        cap_dqbuf.data.sequence,
                        cap_dqbuf.data.index,
                        cap_dqbuf.data.planes[0].bytesused,
                        total_size,
                        fps,
                        num_poll_wakeups as f32 / frame_nb as f32,
                    );
                    io::stdout().flush().unwrap();

                    if let Some(max_cpt) = &mut stop_after {
                        if *max_cpt <= 1 {
                            lets_quit.store(true, Ordering::SeqCst);
                            break;
                        }
                        *max_cpt -= 1;
                    }
                }
            }
        }
    }
    // Insert new line since we were overwriting the same one
    println!();

    client.stop().unwrap();
}

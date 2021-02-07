//! This example program demonstrates how to use the API using the `vicodec`
//! virtual codec driver.
//!
//! There are two variants doing the same thing: one using the higher-level
//! `device` abstraction (used by default), the other using the low-level
//! `ioctl` abstraction (used if `--use_ioctl` is specified).
mod device_api;
mod framegen;
mod ioctl_api;

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::{fs::File, io::Write, sync::Arc};

use clap::{App, Arg};
use v4l2::memory::MemoryType;

fn main() {
    let matches = App::new("vicodec example")
        .arg(
            Arg::with_name("use_ioctl")
                .long("use_ioctl")
                .help("Use the lower-level ioctl interface"),
        )
        .arg(
            Arg::with_name("num_frames")
                .long("stop_after")
                .takes_value(true)
                .help("Stop after encoding this number of buffers"),
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
        .arg(
            Arg::with_name("capture_mem")
                .long("capture_mem")
                .required(false)
                .takes_value(true)
                .default_value("mmap")
                .help("Type of capture memory to use (mmap)"),
        )
        .get_matches();

    let device_path = matches.value_of("device").unwrap_or("/dev/video0");
    let use_ioctl = matches.is_present("use_ioctl");
    let stop_after = match clap::value_t!(matches.value_of("num_frames"), usize) {
        Ok(v) => Some(v),
        Err(e) if e.kind == clap::ErrorKind::ArgumentNotFound => None,
        Err(e) => panic!("Invalid value for stop_after: {}", e),
    };

    let mut output_file = matches
        .value_of("output_file")
        .map(|s| File::create(s).expect("Invalid output file specified."));

    let output_mem = match matches.value_of("output_mem") {
        Some("mmap") => MemoryType::MMAP,
        Some("user") => MemoryType::UserPtr,
        _ => panic!("Invalid value for output_mem"),
    };
    let capture_mem = match matches.value_of("capture_mem") {
        Some("mmap") => MemoryType::MMAP,
        _ => panic!("Invalid value for capture_mem"),
    };

    let save_closure = |b: &[u8]| {
        if let Some(ref mut output) = output_file {
            output
                .write_all(b)
                .expect("Error while writing output data");
        }
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

    if use_ioctl {
        println!("Using ioctl interface");
        ioctl_api::run(
            &Path::new(&device_path),
            output_mem,
            capture_mem,
            lets_quit,
            stop_after,
            save_closure,
        );
    } else {
        println!("Using device interface");
        device_api::run(
            &Path::new(&device_path),
            output_mem,
            capture_mem,
            lets_quit,
            stop_after,
            save_closure,
        )
    }
}

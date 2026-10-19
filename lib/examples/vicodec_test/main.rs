//! This example program demonstrates how to use the API using the `vicodec`
//! virtual codec driver.
//!
//! There are two variants doing the same thing: one using the higher-level
//! `device` abstraction (used by default), the other using the low-level
//! `ioctl` abstraction (used if `--use_ioctl` is specified).
mod device_api;
mod ioctl_api;

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::{fs::File, io::Write, sync::Arc};

use clap::{Arg, Command};
use v4l2r::memory::MemoryType;

fn main() {
    env_logger::init();

    let matches = Command::new("vicodec example")
        .arg(
            Arg::new("use_ioctl")
                .long("use_ioctl")
                .action(clap::ArgAction::SetTrue)
                .help("Use the lower-level ioctl interface"),
        )
        .arg(
            Arg::new("num_frames")
                .long("stop_after")
                .value_parser(clap::value_parser!(usize))
                .help("Stop after encoding this number of buffers"),
        )
        .arg(
            Arg::new("device")
                .default_value("/dev/video0")
                .help("Path to the vicodec device file"),
        )
        .arg(
            Arg::new("output_file")
                .long("save")
                .required(false)
                .help("Save the encoded stream to a file"),
        )
        .arg(
            Arg::new("output_mem")
                .long("output_mem")
                .required(false)
                .value_parser(["mmap", "user"])
                .default_value("user")
                .help("Type of memory to use for the OUTPUT queue"),
        )
        .arg(
            Arg::new("capture_mem")
                .long("capture_mem")
                .required(false)
                .value_parser(["mmap"])
                .default_value("mmap")
                .help("Type of memory to use for the CAPTURE queue"),
        )
        .get_matches();

    let device_path = matches.get_one::<String>("device").unwrap();
    let use_ioctl = matches.get_flag("use_ioctl");
    let stop_after: Option<usize> = matches.get_one("num_frames").map(|n| *n);

    let mut output_file = matches
        .get_one::<String>("output_file")
        .map(|s| File::create(s).expect("Invalid output file specified."));

    let output_mem = match matches.get_one::<String>("output_mem").map(String::as_str) {
        Some("mmap") => MemoryType::Mmap,
        Some("user") => MemoryType::UserPtr,
        _ => panic!("Invalid value for output_mem"),
    };
    let capture_mem = match matches.get_one::<String>("capture_mem").map(String::as_str) {
        Some("mmap") => MemoryType::Mmap,
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
            Path::new(&device_path),
            output_mem,
            capture_mem,
            lets_quit,
            stop_after,
            save_closure,
        );
    } else {
        println!("Using device interface");
        device_api::run(
            Path::new(&device_path),
            output_mem,
            capture_mem,
            lets_quit,
            stop_after,
            save_closure,
        )
    }
}

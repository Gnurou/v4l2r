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
use std::{fs::File, sync::Arc};

use clap::{App, Arg};

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
        .get_matches();

    let device_path = matches.value_of("device").unwrap_or("/dev/video0");
    let use_ioctl = matches.is_present("use_ioctl");
    let stop_after = match clap::value_t!(matches.value_of("num_frames"), usize) {
        Ok(v) => Some(v),
        Err(e) if e.kind == clap::ErrorKind::ArgumentNotFound => None,
        Err(e) => panic!("Invalid value for stop_after: {}", e),
    };

    let output_file: Option<File> = if let Some(path) = matches.value_of("output_file") {
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

    if use_ioctl {
        println!("Using ioctl interface");
        ioctl_api::run(&Path::new(&device_path), lets_quit, stop_after, output_file)
    } else {
        println!("Using device interface");
        device_api::run(&Path::new(&device_path), lets_quit, stop_after, output_file)
    }
}

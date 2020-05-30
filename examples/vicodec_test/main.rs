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
use std::sync::Arc;

use clap::{App, Arg};

fn main() {
    let matches = App::new("vicodec example")
        .arg(
            Arg::with_name("use_ioctl")
                .long("use_ioctl")
                .help("Use the lower-level ioctl interface"),
        )
        .arg(
            Arg::with_name("device")
                .required(true)
                .help("Path to the vicodec device file"),
        )
        .get_matches();

    let device_path = matches.value_of("device").unwrap_or("/dev/video0");
    let use_ioctl = matches.is_present("use_ioctl");

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
        ioctl_api::run(&Path::new(&device_path), lets_quit)
    } else {
        println!("Using device interface");
        device_api::run(&Path::new(&device_path), lets_quit)
    }
}

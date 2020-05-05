mod framegen;
mod ioctl_test;

use ctrlc;
use std::env;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() != 2 {
        println!("Usage: {} /path/to/vicodec/device", args[0]);
        return;
    }

    let lets_quit = Arc::new(AtomicBool::new(false));

    {
        let lets_quit_handler = lets_quit.clone();
        ctrlc::set_handler(move || {
            lets_quit_handler.store(true, Ordering::SeqCst);
        })
        .expect("Failed to set Ctrl-C handler.");
    }

    return ioctl_test::run(&Path::new(&args[1]), lets_quit);
}

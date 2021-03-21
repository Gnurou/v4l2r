//! C FFI of the V4L2R crate.
//!
//! This crate provides a C API that can be used by client programs to make use
//! of the features exported by this crate. For now it strictly focuses on
//! stateful decoders.

pub mod decoder;
pub mod memory;

static INIT: std::sync::Once = std::sync::Once::new();

/// Initialize the V4L2R library. This only sets up the proper hooks for
/// logging, so although it is not a hard requirement to call this function,
/// failure to do so will result in no logs being printed.
#[no_mangle]
pub extern "C" fn v4l2r_init() {
    INIT.call_once(|| {
        #[cfg(feature = "env_logger")]
        env_logger::builder().format_timestamp(None).init();

        #[cfg(feature = "android")]
        android_logger::init_once(
            android_logger::Config::default().with_min_level(log::Level::Trace),
        );
    });
}

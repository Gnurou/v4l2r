use std::env::{self, VarError};
use std::path::PathBuf;

include!("bindgen.rs");

/// Environment variable that can be set to point to the directory containing the `videodev2.h`
/// file to use to generate the bindings.
const V4L2R_VIDEODEV_ENV: &str = "V4L2R_VIDEODEV2_H_PATH";

/// Wrapper file to use as input of bindgen.
const WRAPPER_H: &str = "v4l2r_wrapper.h";

/// Wrapper file used to probe the kernel header version through bindgen.
const VERSION_WRAPPER_H: &str = "v4l2r_version_wrapper.h";

/// Exported constant name from VERSION_WRAPPER_H.
const VERSION_CONST_NAME: &str = "V4L2R_LINUX_VERSION_CODE";

// Fix for https://github.com/rust-lang/rust-bindgen/issues/753
const FIX753_H: &str = "fix753.h";

/// Minimum kernel version code that contains all V4L2 structures used by v4l2r
/// (AV1 stateless codec controls were added in 6.8).
/// 6.8.0 = (6 << 16) | (8 << 8) = 394240
const MIN_KERNEL_VERSION: u64 = (6 << 16) | (8 << 8);

fn builtin_uapi_dir() -> PathBuf {
    PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap()).join("uapi")
}

fn bindgen_arch_args() -> Vec<String> {
    [
        #[cfg(all(feature = "arch64", not(feature = "arch32")))]
        "--target=x86_64-linux-gnu".into(),
        #[cfg(all(feature = "arch32", not(feature = "arch64")))]
        "--target=i686-linux-gnu".into(),
    ]
    .into_iter()
    .collect()
}

fn parse_bindgen_const(bindings: &str, name: &str) -> Option<u64> {
    let prefix = format!("pub const {name}:");

    for line in bindings.lines() {
        if line.starts_with(&prefix) {
            let value = line.split('=').nth(1)?.trim().trim_end_matches(';');
            return value.parse().ok();
        }
    }

    None
}

/// Detect the kernel header version visible to bindgen by parsing a small probe wrapper.
fn target_kernel_version() -> Option<u64> {
    let bindings = bindgen::Builder::default()
        .header(VERSION_WRAPPER_H)
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        .allowlist_var(VERSION_CONST_NAME)
        .layout_tests(false)
        .generate_comments(false)
        .derive_default(false)
        .clang_args(bindgen_arch_args())
        .generate()
        .ok()?;

    parse_bindgen_const(&bindings.to_string(), VERSION_CONST_NAME)
}

/// Decide whether to use the built-in UAPI headers from lib/uapi/.
fn should_use_builtin_uapi() -> bool {
    if cfg!(feature = "builtin-uapi-header") {
        eprintln!("v4l2r: using built-in UAPI headers (feature enabled)");
        return true;
    }
    if let Some(version) = target_kernel_version() {
        if version < MIN_KERNEL_VERSION {
            let (maj, min, pat) = (version >> 16, (version >> 8) & 0xFF, version & 0xFF);
            eprintln!(
                "v4l2r: target kernel headers {}.{}.{} < minimum required; using built-in UAPI headers",
                maj, min, pat
            );
            return true;
        }
    }
    false
}

fn main() {
    println!("cargo::rerun-if-env-changed={}", V4L2R_VIDEODEV_ENV);

    let mut clang_args = bindgen_arch_args();

    let use_builtin = should_use_builtin_uapi();
    if use_builtin && env::var(V4L2R_VIDEODEV_ENV).is_err() {
        // If it is determined that we should use the built-in UAPI headers, and the user has not explicitly set V4L2R_VIDEODEV_ENV,
        // add the built-in UAPI directory to the include path so that bindgen can find the headers.
        let uapi_dir = builtin_uapi_dir();
        clang_args.push(format!("-I{}", uapi_dir.display()));
    } else {
        if let Ok(e) = env::var(V4L2R_VIDEODEV_ENV) {
            // If V4L2R_VIDEODEV_ENV is set, add it to the include path so that bindgen can find the headers.
            clang_args.push(format!("-I{}", e));
            eprintln!(
                "v4l2r: using kernel headers from {} (V4L2R_VIDEODEV_ENV is set)",
                e
            );
        } else {
            // If V4L2R_VIDEODEV_ENV is not set, we rely on the default include paths to find the kernel headers.
            eprintln!("v4l2r: using system kernel headers (V4L2R_VIDEODEV_ENV not set, and built-in UAPI headers not required)");
        }
    }

    let bindings = v4l2r_bindgen_builder(bindgen::Builder::default())
        .header(WRAPPER_H)
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new())) // generate "cargo:rerun-if-changed" directives for all included headers
        .clang_args(clang_args)
        .generate()
        .expect("unable to generate bindings");

    let out_path = PathBuf::from(env::var("OUT_DIR").expect("`OUT_DIR` is not set"));
    bindings
        .write_to_file(out_path.join("bindings.rs"))
        .expect("Couldn't write bindings!");
}

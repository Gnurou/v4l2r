# Rust bindings for V4L2

[![LICENSE](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Build Status](https://github.com/Gnurou/v4l2r/actions/workflows/rust.yml/badge.svg)](https://github.com/Gnurou/v4l2r/actions)
[![Crates.io](https://img.shields.io/crates/v/v4l2r.svg)](https://crates.io/crates/v4l2r)
[![dependency status](https://deps.rs/repo/github/Gnurou/v4l2r/status.svg)](https://deps.rs/repo/github/Gnurou/v4l2r)
[![Documentation](https://docs.rs/v4l2r/badge.svg)](https://docs.rs/v4l2r/)

This is a work-in-progress library to implement safe Rust bindings and high-level
interfaces for V4L2.

Currently the following is implemented:

- Safe low-level abstractions to manage `OUTPUT` and `CAPTURE` queues, as well as
  buffers allocation/queueing/dequeuing for `MMAP`, `USERPTR` and `DMABUF` memory
  types,
- High-level abstraction of the [stateful video decoder
  interface](https://www.kernel.org/doc/html/latest/userspace-api/media/v4l/dev-decoder.html),
- High-level abstraction of the [stateful video encoder
  interface](https://www.kernel.org/doc/html/latest/userspace-api/media/v4l/dev-encoder.html),
- C FFI for using the video decoder interface from C programs.

The library provides several levels of abstraction over V4L2:

- At the lowest level is a very thin layer over the V4L2 ioctls, that stays as
  close as possible to the actual kernel API while adding extra safety and
  removing some of the historical baggage like the difference in format for
  single-planar and multi-planar queues.

- A higher-level abstraction exposes devices, queues, and other V4L2 concepts as
  strongly typed objects. The goal here is to provide an nice-to-use interface
  that remains generic enough to be used for any kind of V4L2 device.

- Finally, more specialized abstractions can be used by applications for
  performing specific tasks, like decoding a video using hardware acceleration.
  For these abstractions, a C FFI is usually provided so their use is not
  limited to Rust.

Dependencies shall be kept to a minimum: this library talks directly to the
kernel using ioctls, and only depends on a few small, well-established crates.

## Project Layout

`lib` contains the Rust library (`v4l2r`), including the thin `ioctl`
abstraction, the more usable `device` abstraction, and task-specific modules for
e.g. video decoding and encoding.

`ffi` contains the C FFI (`v4l2r-ffi`) which is currently exposed as a static
library other projects can link against. A `v4l2r.h` header file with the public
API is generated upon build.

## Build options

`cargo build` will attempt to generate the V4L2 bindings from
`/usr/include/linux/videodev2.h` by default. The `V4L2R_VIDEODEV2_H_PATH`
environment variable can be set to a different location that contains a
`videodev2.h` file if you need to generate the bindings from a different
location.

## How to use

Check `lib/examples/vicodec_test/device_api.rs` for a short example of how to
use the `device`-level interface, or `lib/examples/vicodec_test/ioctl_api.rs`
for the same example using the lower-level `ioctl` API. Both examples encode
generated frames into the `FWHT` format using the `vicodec` kernel driver
(which must be inserted beforehand, using e.g. `modprobe vicodec
multiplanar=1`).

You can try these examples with

    cargo run --example vicodec_test -- /dev/video0

for running the `device` API example, or

    cargo run --example vicodec_test -- /dev/video0 --use_ioctl

for the `ioctl` example, assuming `/dev/video0` is the path to the `vicodec`
encoder.

`lib/examples/fwht_encoder` contains another example program implementing a
higher-level vicodec encoder running in its own thread. It can be run as
follows:

    cargo run --example fwht_encoder -- /dev/video0 --stop_after 20 --save test_encoder.fwht

This invocation will encode 20 generated frames and save the resulting stream in
`test_encoder.fwht`. Pass `--help` to the program for further options.

`lib/examples/simple_decoder` is a decoder example able to decode the streams
produced by the `fwht_encoder` example above, as well as simple Annex-B H.264
streams. For instance, to decode the FWHT stream we just created above:

    cargo run --example simple_decoder -- test_encoder.fwht /dev/video1 --save test_decoder.bgr

`test_decoder.bgr` can be checked with e.g.
[YUView](https://github.com/IENT/YUView). The format will be 640x480 BGR, as
reported by the decoding program.

Finally, `ffi/examples/c_fwht_decode/` contains a C program demonstrating how
to use the C FFI to decode a FWHT stream. See the `Makefile` in that directory
for build and use instructions. The program is purely for demonstration
purposes of the C FII: it is hardcoded to decode the `sample.fwht` file in the
same directory and doesn't support any other output.

## Using in Android

`Android.bp` files are provided that should work on AOSP >= 15. Just check this
repository into `external/rust/crates/v4l2r` and the `libv4l2r` library target
will be available.

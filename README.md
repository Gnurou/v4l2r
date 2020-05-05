# Experimental Rust bindings for V4L2

This is a work-in-progress library to implement safe Rust bindings for V4L2.

**WARNING:** Do not use this for any serious project yet. This is still in the
process of being designed, and also serves as a way for the author to learn
Rust. As such, it cannot be regarded as quality code.

The goal is to make V4L2 accessible to Rust on two level of abstractions:

* A very thin wrapper over the V4L2 ioctls, that stays as close as possible to
  them while adding extra safety and removing some of the historical baggage
  like the difference in format for single-planar and multi-planar queues.

* A higher-level abstraction (still in the design phase) which relies on the
  first one and exposes devices, queues, and other V4L2 concepts as strongly
  typed objects. The goal here is to provide an nice-to-use interface that
  remains generic enough to be used for any kind of V4L2 device.

Other libraries are expected to build upon these abstractions in order to
provide more specialized libraries, e.g. a simple camera or decoder/encoder
library.

Dependencies shall be kept to a minimum: this library talks directly to the
kernel using ioctls, and only depends on a few small, well-established crates.

How to use
----------
Check `examples/vicodec_test/ioctl_test.rs` for a short example of how to use
the low-level ioctl interface. This example program requires the `vicodec`
virtual device, either in single or multi-planar mode.

You can try it with

    cargo run --example vicodec_test -- /dev/video0

assuming `/dev/video0` is the path to the `vicodec` encoder.

//! Represents the possible directions (`OUTPUT` or `CAPTURE`) of a queue.

/// Represents the direction of a `Queue` (`Capture` or `Output`). The direction
/// of a queue limits the operations that are possible on it.
pub trait Direction {}
/// Type for `OUTPUT` queues.
pub struct Output;
impl Direction for Output {}
/// Type for `CAPTURE` queues.
pub struct Capture;
impl Direction for Capture {}

//! Represents the possible directions (`OUTPUT` or `CAPTURE`) of a queue.

use std::fmt::Debug;

/// Represents the direction of a `Queue` (`Capture` or `Output`). The direction
/// of a queue limits the operations that are possible on it.
pub trait Direction: Debug + Send + 'static {}
/// Type for `OUTPUT` queues.
#[derive(Debug)]
pub struct Output;
impl Direction for Output {}
/// Type for `CAPTURE` queues.
#[derive(Debug)]
pub struct Capture;
impl Direction for Capture {}

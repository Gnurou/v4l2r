use super::{PatternSplitter, StreamSplitter};
use std::io;

static H264_START_CODE: [u8; 4] = [0x0, 0x0, 0x0, 0x1];

/// Splits a H.264 annex B stream into chunks that are all guaranteed to contain a full frame
/// worth of data.
///
/// This is a pretty naive implementation that is only guaranteed to work with our examples.
pub struct H264FrameSplitter<S: io::Read>(PatternSplitter<S>);

impl<S: io::Read> H264FrameSplitter<S> {
    pub fn new(stream: S) -> Option<Self> {
        Some(Self(PatternSplitter::new(
            H264_START_CODE.to_vec(),
            stream,
        )?))
    }
}

impl<S: io::Read> Iterator for H264FrameSplitter<S> {
    type Item = Vec<u8>;

    /// Returns the next frame in the stream, header included.
    fn next(&mut self) -> Option<Self::Item> {
        self.0.next()
    }
}

impl<S: io::Read> StreamSplitter for H264FrameSplitter<S> {}

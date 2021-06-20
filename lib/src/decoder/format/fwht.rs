use super::{PatternSplitter, StreamSplitter};
use std::io;

static FRAME_HEADER: [u8; 8] = [0x4f, 0x4f, 0x4f, 0x4f, 0xff, 0xff, 0xff, 0xff];

/// Iterator that returns exactly one frame worth of data from a FWHT stream.
pub struct FwhtFrameParser<S: io::Read>(PatternSplitter<S>);

/// Given a reader, return individual compressed FWHT frames.
impl<S: io::Read> FwhtFrameParser<S> {
    pub fn new(stream: S) -> Option<Self> {
        Some(Self(PatternSplitter::new(FRAME_HEADER.to_vec(), stream)?))
    }
}

impl<S: io::Read> Iterator for FwhtFrameParser<S> {
    type Item = Vec<u8>;

    /// Returns the next frame in the stream, header included.
    fn next(&mut self) -> Option<Self::Item> {
        self.0.next()
    }
}

impl<S: io::Read> StreamSplitter for FwhtFrameParser<S> {}

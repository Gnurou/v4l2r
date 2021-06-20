pub mod fwht;
pub mod h264;

use log::error;
use std::io;

/// Trait for classes able to iterate an encoded stream over chunks of decodable units (typically
/// frames).
pub trait StreamSplitter: Iterator<Item = Vec<u8>> {}

/// Splits a stream at each encounter of a given pattern. Useful to extract decodable units (or
/// frames from an encoded stream.
struct PatternSplitter<S: io::Read> {
    /// The pattern to split at.
    pattern: Vec<u8>,
    stream: io::Bytes<S>,
}

impl<S: io::Read> PatternSplitter<S> {
    /// Create a new splitter that will split `stream` at each instance of `pattern`.
    ///
    /// `stream` must start with `pattern`, otherwise the input is considered invalid and `None` is
    /// returned.
    fn new(pattern: impl Into<Vec<u8>>, stream: S) -> Option<Self> {
        let mut stream = stream.bytes();
        let pattern = pattern.into();

        // The stream must begin by our header, or it is invalid.
        let stream_start = (0..pattern.len())
            .into_iter()
            .map(|_| stream.next().unwrap_or(Ok(0)).unwrap_or(0))
            .collect::<Vec<_>>();

        if stream_start == pattern {
            Some(PatternSplitter { pattern, stream })
        } else {
            None
        }
    }
}

impl<S: io::Read> Iterator for PatternSplitter<S> {
    type Item = Vec<u8>;

    /// Returns the next frame in the stream, header included.
    fn next(&mut self) -> Option<Self::Item> {
        let mut frame_data: Vec<u8> = Vec::with_capacity(0x10000);
        // Add the header of the frame since we are already past it.
        frame_data.extend(&self.pattern);
        // Window used to try and detect the next frame header.
        let mut header_window: Vec<u8> = Vec::with_capacity(self.pattern.len());
        // Iterator to the next character expected for a frame header.
        let mut header_iter = self.pattern.iter().peekable();

        loop {
            let b = match self.stream.next() {
                Some(Ok(b)) => b,
                // End of stream, commit all read data and return.
                None => {
                    frame_data.extend(&header_window);
                    // If the only data is our pre-filled header, then we haven't
                    // read anything and thus there is no frame to return.
                    return if frame_data.len() <= self.pattern.len() {
                        None
                    } else {
                        Some(frame_data)
                    };
                }
                Some(Err(e)) => {
                    error!("Error while reading stream: {}", e);
                    return None;
                }
            };

            // Add the read byte to the header candidate buffer.
            header_window.push(b);

            // Possibly a header?
            if Some(&&b) == header_iter.peek() {
                header_iter.next();
                // Found next frame's header, return read data.
                if header_iter.peek().is_none() {
                    return Some(frame_data);
                }
            } else {
                // Not a header, commit header window data to current frame and reset.
                frame_data.extend(&header_window);
                if header_window.len() > 1 {
                    header_iter = self.pattern.iter().peekable();
                }
                header_window.clear();
            }
        }
    }
}

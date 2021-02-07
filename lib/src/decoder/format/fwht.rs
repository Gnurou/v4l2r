use std::{io, slice};

pub struct FwhtFrameParser<S: io::Read> {
    stream: S,
}

static FRAME_HEADER: [u8; 8] = [0x4f, 0x4f, 0x4f, 0x4f, 0xff, 0xff, 0xff, 0xff];

/// Given a reader, return individual compressed FWHT frames.
impl<S: io::Read> FwhtFrameParser<S> {
    /// Create a new FWHT parser, ready to return the first compressed frame.
    /// `stream` will be moved to the first detected FWHT frame.
    /// Returns None if no FWHT frame can be detected in the whole stream.
    pub fn new(stream: S) -> Option<Self> {
        let mut parser = FwhtFrameParser { stream };

        // Catch the first frame header, if any.
        parser.next()?;
        Some(parser)
    }
}

impl<S: io::Read> Iterator for FwhtFrameParser<S> {
    type Item = Vec<u8>;

    /// Returns the next frame in the stream, header included.
    fn next(&mut self) -> Option<Self::Item> {
        let mut frame_data: Vec<u8> = Vec::with_capacity(0x10000);
        // Add the header of the frame since we are already past it.
        frame_data.extend(&FRAME_HEADER);
        // Window used to try and detect the next frame header.
        let mut header_window: Vec<u8> = Vec::with_capacity(FRAME_HEADER.len());
        // Iterator to the next character expected for a frame header.
        let mut header_iter = FRAME_HEADER.iter().peekable();

        loop {
            let mut b = 0u8;
            match self.stream.read_exact(slice::from_mut(&mut b)) {
                Ok(()) => (),
                // End of stream, commit all read data and return.
                Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => {
                    frame_data.extend(&header_window);
                    // If the only data is our pre-filled header, then we haven't
                    // read anything and thus there is no frame to return.
                    return if frame_data.len() <= FRAME_HEADER.len() {
                        None
                    } else {
                        Some(frame_data)
                    };
                }
                Err(e) => {
                    eprintln!("error while reading FWHT stream: {}", e);
                    return None;
                }
            }

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
                    header_iter = FRAME_HEADER.iter().peekable();
                }
                header_window.clear();
            }
        }
    }
}

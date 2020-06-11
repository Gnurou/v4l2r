use std::error;
use std::fmt;

#[derive(Debug)]
pub enum Error {
    InvalidStride,
    BufferTooSmall,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::InvalidStride => write!(f, "Stride is smaller than width"),
            Error::BufferTooSmall => write!(f, "Buffer too small"),
        }
    }
}

impl error::Error for Error {}

pub struct FrameGenerator {
    width: usize,
    height: usize,
    stride: usize,
    step: u32,
}

impl FrameGenerator {
    pub fn new(width: usize, height: usize, stride: usize) -> Result<Self, Error> {
        if stride < width * 3 {
            return Err(Error::InvalidStride);
        }

        Ok(FrameGenerator {
            width,
            height,
            stride,
            step: 0,
        })
    }

    pub fn next_frame(&mut self, frame: &mut [u8]) -> Result<(), Error> {
        if frame.len() < self.stride * self.height {
            return Err(Error::BufferTooSmall);
        }

        self.gen_pattern(frame);
        self.step = self.step.wrapping_add(1);

        Ok(())
    }

    fn gen_pattern(&mut self, frame: &mut [u8]) {
        for y in 0..self.height {
            let line_offset = y * self.stride;
            let line = &mut frame[line_offset..line_offset + self.width * 3];
            for x in 0..self.width {
                let pixel = &mut line[x * 3..(x + 1) * 3];
                let rgba = self.step.wrapping_add((x ^ y) as u32).to_le_bytes();
                pixel[0] = rgba[0];
                pixel[1] = rgba[1];
                pixel[2] = rgba[2];
            }
        }
    }
}

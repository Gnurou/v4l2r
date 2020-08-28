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
        frame
            .chunks_exact_mut(self.stride)
            .map(|l| &mut l[0..self.width * 3])
            .enumerate()
            .for_each(|(y, line)| {
                line.chunks_exact_mut(3).enumerate().for_each(|(x, pixel)| {
                    let rgba = self.step.wrapping_add((x ^ y) as u32).to_le_bytes();
                    pixel[0] = rgba[0];
                    pixel[1] = rgba[1];
                    pixel[2] = rgba[2];
                });
            });
    }
}

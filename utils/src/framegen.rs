use thiserror::Error;
#[derive(Debug, Error)]
pub enum NewFrameGeneratorError {
    #[error("Invalid stride")]
    InvalidStride,
}

#[derive(Debug, Error)]
pub enum GenerateFrameError {
    #[error("Provided buffer is too small")]
    BufferTooSmall,
}

pub struct FrameGenerator {
    width: usize,
    height: usize,
    stride: usize,
    step: u32,
}

impl FrameGenerator {
    pub fn new(width: usize, height: usize, stride: usize) -> Result<Self, NewFrameGeneratorError> {
        if stride < width * 3 {
            return Err(NewFrameGeneratorError::InvalidStride);
        }

        Ok(FrameGenerator {
            width,
            height,
            stride,
            step: 0,
        })
    }

    pub fn frame_size(&self) -> usize {
        self.stride * self.height
    }

    pub fn next_frame<S: AsMut<[u8]>>(&mut self, frame: &mut S) -> Result<(), GenerateFrameError> {
        let frame = frame.as_mut();

        if frame.len() < self.frame_size() {
            return Err(GenerateFrameError::BufferTooSmall);
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

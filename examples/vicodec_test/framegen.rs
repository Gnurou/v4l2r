//! Simple image pattern generator, inspired by <http://cliffle.com/blog/bare-metal-wasm/>

/// Generate a pattern in `frame`, filling as many lines as can be using
/// `bytes_per_line. `seed` can be increased over consecutive calls to animate
/// the pattern.
pub fn gen_pattern(frame: &mut [u8], bytes_per_line: usize, seed: u32) {
    let width = bytes_per_line / 3;
    let height = frame.len() / bytes_per_line;

    (0..height)
        .flat_map(move |y| (0..width).map(move |x| (x, y)))
        .zip(frame.chunks_mut(3))
        .for_each(|((x, y), pixel)| {
            let rgba = seed.wrapping_add((x ^ y) as u32).to_le_bytes();
            pixel[0] = rgba[0];
            pixel[1] = rgba[1];
            pixel[2] = rgba[2];
        });
}

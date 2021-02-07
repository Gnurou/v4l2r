//! Simple image pattern generator, inspired by <http://cliffle.com/blog/bare-metal-wasm/>

/// Generate a pattern in `frame`, filling as many lines as can be using
/// `bytes_per_line. `seed` can be increased over consecutive calls to animate
/// the pattern.
pub fn gen_pattern<S: AsMut<[u8]>>(frame: &mut S, bytes_per_line: usize, seed: u32) {
    frame
        .as_mut()
        .chunks_exact_mut(bytes_per_line)
        .enumerate()
        .for_each(|(y, line)| {
            line.chunks_exact_mut(3).enumerate().for_each(|(x, pixel)| {
                let rgba = seed.wrapping_add((x ^ y) as u32).to_le_bytes();
                pixel[0] = rgba[0];
                pixel[1] = rgba[1];
                pixel[2] = rgba[2];
            });
        });
}

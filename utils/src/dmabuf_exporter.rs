use std::fs::File;

use nix::libc::off_t;

use nix::{
    sys::memfd::{memfd_create, MemFdCreateFlag},
    unistd::ftruncate,
};
use v4l2r::{memory::DmaBufHandle, Format};

pub fn export_dmabufs(
    format: &Format,
    nb_buffers: usize,
) -> nix::Result<Vec<Vec<DmaBufHandle<File>>>> {
    let fds: Vec<Vec<DmaBufHandle<File>>> = (0..nb_buffers)
        .map(|_| {
            format
                .plane_fmt
                .iter()
                .map(|plane| {
                    memfd_create(c"memfd buffer", MemFdCreateFlag::MFD_ALLOW_SEALING)
                        .and_then(|fd| ftruncate(&fd, plane.sizeimage as off_t).map(|_| fd))
                        .map(|fd| DmaBufHandle::from(File::from(fd)))
                })
                .collect::<Result<Vec<_>, _>>()
        })
        .collect::<Result<Vec<_>, _>>()?;

    Ok(fds)
}

#[cfg(test)]
mod tests {
    use v4l2r::{memory::DmaBufSource, Format, PixelFormat, PlaneLayout};

    use super::export_dmabufs;

    #[test]
    fn test_export_dmabufs() {
        const WIDTH: u32 = 640;
        const HEIGHT: u32 = 480;
        const BYTES_PER_LINE: u32 = WIDTH;
        const SIZE_IMAGE_Y: u32 = BYTES_PER_LINE * HEIGHT;
        const SIZE_IMAGE_UV: u32 = BYTES_PER_LINE * HEIGHT / 2;

        let format = Format {
            width: WIDTH,
            height: HEIGHT,
            pixelformat: PixelFormat::from_fourcc(b"NV12"),
            plane_fmt: vec![
                PlaneLayout {
                    sizeimage: SIZE_IMAGE_Y,
                    bytesperline: BYTES_PER_LINE,
                },
                PlaneLayout {
                    sizeimage: SIZE_IMAGE_UV,
                    bytesperline: BYTES_PER_LINE,
                },
            ],
        };

        let dmabufs = export_dmabufs(&format, 4).unwrap();
        assert_eq!(dmabufs.len(), 4);
        for buf in dmabufs {
            assert_eq!(buf.len(), 2);
            assert!(buf[0].0.len() >= SIZE_IMAGE_Y as u64);
            assert!(buf[1].0.len() >= SIZE_IMAGE_UV as u64);
        }
    }
}

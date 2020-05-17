use super::ioctl;
use super::ioctl::Capability;
use super::QueueType;
use super::Result;
use fd::FileDesc;
use std::cell::RefCell;
use std::collections::BTreeSet;
use std::os::unix::io::{AsRawFd, RawFd};
use std::path::Path;

mod queue;
pub use queue::*;

#[derive(Default)]
pub struct DeviceConfig {
    non_blocking_dqbuf: bool,
}

impl DeviceConfig {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn non_blocking_dqbuf(self) -> Self {
        DeviceConfig {
            non_blocking_dqbuf: true,
            ..self
        }
    }
}

pub struct Device {
    pub capability: Capability,
    fd: RefCell<FileDesc>,
    used_queues: RefCell<BTreeSet<QueueType>>,
}

impl Device {
    fn new(fd: FileDesc) -> Result<Self> {
        Ok(Device {
            capability: ioctl::querycap(&fd)?,
            fd: RefCell::new(fd),
            used_queues: RefCell::new(BTreeSet::new()),
        })
    }

    pub fn open(path: &Path, config: DeviceConfig) -> Result<Self> {
        use nix::fcntl::{open, OFlag};
        use nix::sys::stat::Mode;

        let flags = OFlag::O_RDWR
            | OFlag::O_CLOEXEC
            | match config.non_blocking_dqbuf {
                true => OFlag::O_NONBLOCK,
                false => OFlag::empty(),
            };

        Device::new(FileDesc::new(open(path, flags, Mode::empty())?, true))
    }
}

impl AsRawFd for Device {
    fn as_raw_fd(&self) -> RawFd {
        self.fd.borrow().as_raw_fd()
    }
}

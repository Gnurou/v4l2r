//! Provides an interface to the V4L2 primitives that is both safe and
//! higher-level than `ioctl`, while staying low-level enough to allow the
//! implementation of any kind of V4L2 program on top of it.
//!
//! The `Device` struct lets the user open a V4L2 device and querying its
//! capabilities, from which `Queue` objects can be created.
//!
//! A `Queue` object can be assigned a format and allocated buffers, from which
//! point it can be streamed on and off, and buffers queued to it.
//!
//! The emphasis of this interface is to limit the actions and data that are
//! accessible at a given point in time to those that make sense. For instance,
//! the compiler wil reject any code that tries to stream a queue on before it
//! has buffers allocated. Similarly, if a `Queue` uses `UserPtr` buffers, then
//! queuing a buffer requires to provide a valid memory area to back it up.
//!
//! Using this interface, the user does not have to worry about which fields of
//! a V4L2 structure make sense - if it is relevant, then it will be visible,
//! and if it is required, then the code won't compile unless it is provided.
use super::ioctl;
use super::ioctl::Capability;
use super::QueueType;
use super::Result;
use std::collections::BTreeSet;
use std::fs::File;
use std::os::unix::io::{AsRawFd, FromRawFd, RawFd};
use std::path::Path;

pub mod queue;

/// Options that can be specified when creating a `Device`.
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
        }
    }
}

/// An opened V4L2 device. `Queue` objects can be instantiated from it.
pub struct Device {
    pub capability: Capability,
    fd: File,
    used_queues: BTreeSet<QueueType>,
}

impl Device {
    fn new(fd: File) -> Result<Self> {
        Ok(Device {
            capability: ioctl::querycap(&fd)?,
            fd,
            used_queues: BTreeSet::new(),
        })
    }

    pub fn open(path: &Path, config: DeviceConfig) -> Result<Self> {
        use nix::fcntl::{open, OFlag};
        use nix::sys::stat::Mode;

        let flags = OFlag::O_RDWR
            | OFlag::O_CLOEXEC
            | if config.non_blocking_dqbuf {
                OFlag::O_NONBLOCK
            } else {
                OFlag::empty()
            };

        let fd = open(path, flags, Mode::empty())?;

        // Safe because we are constructing a file from Fd we just opened.
        Device::new(unsafe { File::from_raw_fd(fd) })
    }
}

impl AsRawFd for Device {
    fn as_raw_fd(&self) -> RawFd {
        self.fd.as_raw_fd()
    }
}

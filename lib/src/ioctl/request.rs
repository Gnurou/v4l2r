use nix::libc::c_int;
use nix::poll::{poll, PollFd};
use std::fs::File;
use std::os::unix::io::{AsFd, AsRawFd, RawFd};
use std::os::unix::prelude::FromRawFd;
use thiserror::Error;

pub use nix::poll::PollFlags;

#[doc(hidden)]
mod ioctl {
    use nix::libc::c_int;
    nix::ioctl_read!(media_ioc_request_alloc, b'|', 5, c_int);
    nix::ioctl_none!(media_request_ioc_queue, b'|', 0x80);
    nix::ioctl_none!(media_request_ioc_reinit, b'|', 0x81);
}

#[derive(Debug, Error)]
pub enum RequestError {
    #[error("Unexpected ioctl error: {0}")]
    IoctlError(nix::Error),
    #[error("Unknown poll flag returned")]
    UnknownPollFlagReturned,
}

#[derive(Debug)]
pub struct Request {
    fd: File,
}

impl Request {
    pub fn alloc(media_fd: &impl AsRawFd) -> Result<Self, RequestError> {
        let mut request_fd: RawFd = 0;
        // SAFETY: the 'data' argument is an address of a variable compatible with a C integer
        match unsafe {
            ioctl::media_ioc_request_alloc(
                media_fd.as_raw_fd(),
                &mut request_fd as *mut _ as *mut c_int,
            )
        } {
            // SAFETY: correctly executed ioctl guarantees request_fd is a correct file descriptor
            Ok(_) => Ok(Request {
                fd: unsafe { File::from_raw_fd(request_fd) },
            }),
            Err(e) => Err(RequestError::IoctlError(e)),
        }
    }

    pub fn queue(&self) -> Result<(), RequestError> {
        // SAFETY: the file descriptor is correct
        match unsafe { ioctl::media_request_ioc_queue(self.as_raw_fd()) } {
            Ok(_) => Ok(()),
            Err(e) => Err(RequestError::IoctlError(e)),
        }
    }

    pub fn reinit(&self) -> Result<(), RequestError> {
        // SAFETY: the file descriptor is correct
        match unsafe { ioctl::media_request_ioc_reinit(self.as_raw_fd()) } {
            Ok(_) => Ok(()),
            Err(e) => Err(RequestError::IoctlError(e)),
        }
    }

    pub fn poll(
        &self,
        video_fd: &impl AsFd,
        events: PollFlags,
        timeout: Option<u32>,
    ) -> Result<PollFlags, RequestError> {
        let mut poll_fd = [PollFd::new(video_fd, events)];
        let timeout = timeout.map_or(-1, |v| v as i32);

        match poll(&mut poll_fd, timeout) {
            Ok(0) => Ok(PollFlags::empty()),
            Ok(_) => {
                if let Some(poll_flags) = poll_fd[0].revents() {
                    Ok(poll_flags)
                } else {
                    Err(RequestError::UnknownPollFlagReturned)
                }
            }
            Err(e) => Err(RequestError::IoctlError(e)),
        }
    }
}

impl AsRawFd for Request {
    fn as_raw_fd(&self) -> RawFd {
        self.fd.as_raw_fd()
    }
}

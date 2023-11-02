use std::mem;
use std::os::unix::io::AsRawFd;

use bitflags::bitflags;
use enumn::N;
use nix::errno::Errno;
use thiserror::Error;

use crate::bindings;
use crate::bindings::v4l2_audio;
use crate::bindings::v4l2_audioout;

bitflags! {
    #[derive(Clone, Copy, Debug)]
    pub struct AudioCapability: u32 {
        const STEREO = bindings::V4L2_AUDCAP_STEREO;
        const AVL = bindings::V4L2_AUDCAP_AVL;
    }
}

#[derive(Debug, N)]
#[repr(u32)]
pub enum AudioMode {
    Avl = bindings::V4L2_AUDMODE_AVL,
}

#[doc(hidden)]
mod ioctl {
    use crate::bindings::v4l2_audio;
    use crate::bindings::v4l2_audioout;

    nix::ioctl_read!(vidioc_g_audio, b'V', 33, v4l2_audio);
    nix::ioctl_write_ptr!(vidioc_s_audio, b'V', 34, v4l2_audio);
    nix::ioctl_read!(vidioc_g_audout, b'V', 49, v4l2_audioout);
    nix::ioctl_write_ptr!(vidioc_s_audout, b'V', 50, v4l2_audioout);
    nix::ioctl_readwrite!(vidioc_enumaudio, b'V', 65, v4l2_audio);
    nix::ioctl_readwrite!(vidioc_enumaudout, b'V', 66, v4l2_audioout);
}

#[derive(Debug, Error)]
pub enum GAudioError {
    #[error("invalid audio input")]
    Invalid,
    #[error("ioctl error: {0}")]
    IoctlError(Errno),
}

impl From<GAudioError> for Errno {
    fn from(err: GAudioError) -> Self {
        match err {
            GAudioError::Invalid => Errno::EINVAL,
            GAudioError::IoctlError(e) => e,
        }
    }
}

/// Safe wrapper around the `VIDIOC_G_AUDIO` ioctl.
pub fn g_audio<O: From<v4l2_audio>>(fd: &impl AsRawFd) -> Result<O, GAudioError> {
    let mut audio = v4l2_audio {
        ..unsafe { mem::zeroed() }
    };

    match unsafe { ioctl::vidioc_g_audio(fd.as_raw_fd(), &mut audio) } {
        Ok(_) => Ok(O::from(audio)),
        Err(Errno::EINVAL) => Err(GAudioError::Invalid),
        Err(e) => Err(GAudioError::IoctlError(e)),
    }
}

/// Safe wrapper around the `VIDIOC_S_AUDIO` ioctl.
pub fn s_audio(fd: &impl AsRawFd, index: u32, mode: AudioMode) -> Result<(), GAudioError> {
    let audio = v4l2_audio {
        index,
        mode: mode as u32,
        ..unsafe { mem::zeroed() }
    };

    match unsafe { ioctl::vidioc_s_audio(fd.as_raw_fd(), &audio) } {
        Ok(_) => Ok(()),
        Err(Errno::EINVAL) => Err(GAudioError::Invalid),
        Err(e) => Err(GAudioError::IoctlError(e)),
    }
}

/// Safe wrapper around the `VIDIOC_G_AUDOUT` ioctl.
pub fn g_audout<O: From<v4l2_audioout>>(fd: &impl AsRawFd) -> Result<O, GAudioError> {
    let mut audio = v4l2_audioout {
        ..unsafe { mem::zeroed() }
    };

    match unsafe { ioctl::vidioc_g_audout(fd.as_raw_fd(), &mut audio) } {
        Ok(_) => Ok(O::from(audio)),
        Err(Errno::EINVAL) => Err(GAudioError::Invalid),
        Err(e) => Err(GAudioError::IoctlError(e)),
    }
}

/// Safe wrapper around the `VIDIOC_S_AUDIO` ioctl.
pub fn s_audout(fd: &impl AsRawFd, index: u32) -> Result<(), GAudioError> {
    let audio = v4l2_audioout {
        index,
        ..unsafe { mem::zeroed() }
    };

    match unsafe { ioctl::vidioc_s_audout(fd.as_raw_fd(), &audio) } {
        Ok(_) => Ok(()),
        Err(Errno::EINVAL) => Err(GAudioError::Invalid),
        Err(e) => Err(GAudioError::IoctlError(e)),
    }
}

/// Safe wrapper around the `VIDIOC_ENUMAUDIO` ioctl.
pub fn enumaudio<O: From<v4l2_audio>>(fd: &impl AsRawFd, index: u32) -> Result<O, GAudioError> {
    let mut audio = v4l2_audio {
        index,
        ..unsafe { mem::zeroed() }
    };

    match unsafe { ioctl::vidioc_enumaudio(fd.as_raw_fd(), &mut audio) } {
        Ok(_) => Ok(O::from(audio)),
        Err(Errno::EINVAL) => Err(GAudioError::Invalid),
        Err(e) => Err(GAudioError::IoctlError(e)),
    }
}

/// Safe wrapper around the `VIDIOC_ENUMAUDOUT` ioctl.
pub fn enumaudout<O: From<v4l2_audioout>>(fd: &impl AsRawFd, index: u32) -> Result<O, GAudioError> {
    let mut audio = v4l2_audioout {
        index,
        ..unsafe { mem::zeroed() }
    };

    match unsafe { ioctl::vidioc_enumaudout(fd.as_raw_fd(), &mut audio) } {
        Ok(_) => Ok(O::from(audio)),
        Err(Errno::EINVAL) => Err(GAudioError::Invalid),
        Err(e) => Err(GAudioError::IoctlError(e)),
    }
}

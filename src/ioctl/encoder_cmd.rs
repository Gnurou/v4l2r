use crate::bindings;
use std::{mem, os::unix::io::AsRawFd};
use thiserror::Error;

#[doc(hidden)]
mod ioctl {
    use crate::bindings::v4l2_encoder_cmd;
    nix::ioctl_readwrite!(vidioc_encoder_cmd, b'V', 77, v4l2_encoder_cmd);
}

pub enum EncoderCommand {
    Start,
    Stop(bool),
    Pause,
    Resume,
}

#[derive(Debug, Error)]
pub enum EncoderCmdError {
    #[error("Drain already in progress")]
    DrainInProgress,
    #[error("Command not supported by device")]
    UnsupportedCommand,
    #[error("Unexpected ioctl error: {0}")]
    IoctlError(nix::Error),
}

pub fn encoder_cmd(fd: &impl AsRawFd, command: EncoderCommand) -> Result<(), EncoderCmdError> {
    let mut enc_cmd = bindings::v4l2_encoder_cmd {
        cmd: match &command {
            EncoderCommand::Start => bindings::V4L2_ENC_CMD_START,
            EncoderCommand::Stop(_) => bindings::V4L2_ENC_CMD_STOP,
            EncoderCommand::Pause => bindings::V4L2_ENC_CMD_PAUSE,
            EncoderCommand::Resume => bindings::V4L2_ENC_CMD_RESUME,
        },
        flags: match &command {
            EncoderCommand::Stop(at_gop) if *at_gop => bindings::V4L2_ENC_CMD_STOP_AT_GOP_END,
            _ => 0,
        },
        ..unsafe { mem::zeroed() }
    };

    match unsafe { ioctl::vidioc_encoder_cmd(fd.as_raw_fd(), &mut enc_cmd) } {
        Ok(_) => Ok(()),
        Err(nix::Error::Sys(nix::errno::Errno::EBUSY)) => Err(EncoderCmdError::DrainInProgress),
        Err(nix::Error::Sys(nix::errno::Errno::EINVAL)) => Err(EncoderCmdError::UnsupportedCommand),
        Err(e) => Err(EncoderCmdError::IoctlError(e)),
    }
}

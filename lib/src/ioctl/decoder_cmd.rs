use crate::bindings;
use nix::errno::Errno;
use nix::Error;
use std::{mem, os::unix::io::AsRawFd};
use thiserror::Error;

#[doc(hidden)]
mod ioctl {
    use crate::bindings::v4l2_decoder_cmd;
    nix::ioctl_readwrite!(vidioc_decoder_cmd, b'V', 96, v4l2_decoder_cmd);
    nix::ioctl_readwrite!(vidioc_try_decoder_cmd, b'V', 97, v4l2_decoder_cmd);
}

#[derive(Debug, Clone, Copy)]
pub enum DecoderCommand {
    // TODO: support extra data
    Start,
    // TODO: support extra data
    Stop,
    Pause,
    Resume,
}

#[derive(Debug, Error)]
pub enum DecoderCmdError {
    #[error("Drain already in progress")]
    DrainInProgress,
    #[error("Command not supported by device")]
    UnsupportedCommand(DecoderCommand),
    #[error("Unexpected ioctl error: {0}")]
    IoctlError(Error),
}

fn map_nix_error(error: nix::Error, command: DecoderCommand) -> DecoderCmdError {
    match error {
        Error::Sys(Errno::EBUSY) => DecoderCmdError::DrainInProgress,
        Error::Sys(Errno::EINVAL) => DecoderCmdError::UnsupportedCommand(command),
        e => DecoderCmdError::IoctlError(e),
    }
}

impl From<DecoderCommand> for bindings::v4l2_decoder_cmd {
    fn from(command: DecoderCommand) -> Self {
        bindings::v4l2_decoder_cmd {
            cmd: match &command {
                DecoderCommand::Start => bindings::V4L2_DEC_CMD_START,
                DecoderCommand::Stop => bindings::V4L2_DEC_CMD_STOP,
                DecoderCommand::Pause => bindings::V4L2_DEC_CMD_PAUSE,
                DecoderCommand::Resume => bindings::V4L2_DEC_CMD_RESUME,
            },
            ..unsafe { mem::zeroed() }
        }
    }
}

pub fn decoder_cmd(fd: &impl AsRawFd, command: DecoderCommand) -> Result<(), DecoderCmdError> {
    let mut dec_cmd = bindings::v4l2_decoder_cmd::from(command);

    match unsafe { ioctl::vidioc_decoder_cmd(fd.as_raw_fd(), &mut dec_cmd) } {
        Ok(_) => Ok(()),
        Err(e) => Err(map_nix_error(e, command)),
    }
}

pub fn try_decoder_cmd(fd: &impl AsRawFd, command: DecoderCommand) -> Result<(), DecoderCmdError> {
    let mut dec_cmd = bindings::v4l2_decoder_cmd::from(command);

    match unsafe { ioctl::vidioc_try_decoder_cmd(fd.as_raw_fd(), &mut dec_cmd) } {
        Ok(_) => Ok(()),
        Err(e) => Err(map_nix_error(e, command)),
    }
}

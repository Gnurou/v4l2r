use crate::bindings;
use nix::errno::Errno;
use std::{mem, os::unix::io::AsRawFd};
use thiserror::Error;

#[doc(hidden)]
mod ioctl {
    use crate::bindings::v4l2_encoder_cmd;
    nix::ioctl_readwrite!(vidioc_encoder_cmd, b'V', 77, v4l2_encoder_cmd);
    nix::ioctl_readwrite!(vidioc_try_encoder_cmd, b'V', 78, v4l2_encoder_cmd);
}

#[derive(Debug, Clone, Copy)]
pub enum EncoderCommand {
    Start,
    Stop(bool),
    Pause,
    Resume,
}

#[derive(Debug, Error)]
pub enum EncoderCmdError {
    #[error("drain already in progress")]
    DrainInProgress,
    #[error("command not supported by device")]
    UnsupportedCommand,
    #[error("ioctl error: {0}")]
    IoctlError(Errno),
}

impl From<EncoderCmdError> for Errno {
    fn from(err: EncoderCmdError) -> Self {
        match err {
            EncoderCmdError::DrainInProgress => Errno::EBUSY,
            EncoderCmdError::UnsupportedCommand => Errno::EINVAL,
            EncoderCmdError::IoctlError(e) => e,
        }
    }
}

impl From<Errno> for EncoderCmdError {
    fn from(error: Errno) -> Self {
        match error {
            Errno::EBUSY => EncoderCmdError::DrainInProgress,
            Errno::EINVAL => EncoderCmdError::UnsupportedCommand,
            e => EncoderCmdError::IoctlError(e),
        }
    }
}

impl From<&EncoderCommand> for bindings::v4l2_encoder_cmd {
    fn from(command: &EncoderCommand) -> Self {
        bindings::v4l2_encoder_cmd {
            cmd: match command {
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
        }
    }
}

pub fn encoder_cmd<I: Into<bindings::v4l2_encoder_cmd>>(
    fd: &impl AsRawFd,
    command: I,
) -> Result<(), EncoderCmdError> {
    let mut enc_cmd = command.into();

    match unsafe { ioctl::vidioc_encoder_cmd(fd.as_raw_fd(), &mut enc_cmd) } {
        Ok(_) => Ok(()),
        Err(e) => Err(e.into()),
    }
}

pub fn try_encoder_cmd<I: Into<bindings::v4l2_encoder_cmd>>(
    fd: &impl AsRawFd,
    command: I,
) -> Result<(), EncoderCmdError> {
    let mut enc_cmd = command.into();

    match unsafe { ioctl::vidioc_try_encoder_cmd(fd.as_raw_fd(), &mut enc_cmd) } {
        Ok(_) => Ok(()),
        Err(e) => Err(e.into()),
    }
}

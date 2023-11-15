use std::convert::{Infallible, TryFrom};
use std::mem;
use std::os::unix::io::AsRawFd;

use nix::errno::Errno;
use thiserror::Error;

use crate::bindings;
use crate::bindings::v4l2_enc_idx;
use crate::bindings::v4l2_encoder_cmd;

#[doc(hidden)]
mod ioctl {
    use crate::bindings::v4l2_enc_idx;
    use crate::bindings::v4l2_encoder_cmd;

    nix::ioctl_read!(vidioc_g_enc_index, b'V', 76, v4l2_enc_idx);
    nix::ioctl_readwrite!(vidioc_encoder_cmd, b'V', 77, v4l2_encoder_cmd);
    nix::ioctl_readwrite!(vidioc_try_encoder_cmd, b'V', 78, v4l2_encoder_cmd);
}

#[derive(Debug, Error)]
pub enum GEncIndexError {
    #[error("ioctl error: {0}")]
    IoctlError(Errno),
}

impl From<GEncIndexError> for Errno {
    fn from(err: GEncIndexError) -> Self {
        match err {
            GEncIndexError::IoctlError(e) => e,
        }
    }
}

/// Safe wrapper around the `VIDIOC_G_ENC_INDEX` ioctl.
pub fn g_enc_index<O: From<v4l2_enc_idx>>(fd: &impl AsRawFd) -> Result<O, GEncIndexError> {
    let mut enc_idx: v4l2_enc_idx = unsafe { std::mem::zeroed() };

    match unsafe { ioctl::vidioc_g_enc_index(fd.as_raw_fd(), &mut enc_idx) } {
        Ok(_) => Ok(O::from(enc_idx)),
        Err(e) => Err(GEncIndexError::IoctlError(e)),
    }
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
    #[error("error while converting from v4l2_encoder_cmd")]
    FromV4L2CommandConversionError,
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
            EncoderCmdError::FromV4L2CommandConversionError => Errno::EINVAL,
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

impl From<&EncoderCommand> for v4l2_encoder_cmd {
    fn from(command: &EncoderCommand) -> Self {
        v4l2_encoder_cmd {
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

impl TryFrom<v4l2_encoder_cmd> for () {
    type Error = Infallible;

    fn try_from(_: v4l2_encoder_cmd) -> Result<Self, Self::Error> {
        Ok(())
    }
}

/// Safe wrapper around the `VIDIOC_ENCODER_CMD` ioctl.
pub fn encoder_cmd<I: Into<v4l2_encoder_cmd>, O: TryFrom<v4l2_encoder_cmd>>(
    fd: &impl AsRawFd,
    command: I,
) -> Result<O, EncoderCmdError> {
    let mut enc_cmd = command.into();

    match unsafe { ioctl::vidioc_encoder_cmd(fd.as_raw_fd(), &mut enc_cmd) } {
        Ok(_) => Ok(
            O::try_from(enc_cmd).map_err(|_| EncoderCmdError::FromV4L2CommandConversionError)?
        ),
        Err(e) => Err(e.into()),
    }
}

/// Safe wrapper around the `VIDIOC_TRY_ENCODER_CMD` ioctl.
pub fn try_encoder_cmd<I: Into<v4l2_encoder_cmd>, O: TryFrom<v4l2_encoder_cmd>>(
    fd: &impl AsRawFd,
    command: I,
) -> Result<O, EncoderCmdError> {
    let mut enc_cmd = command.into();

    match unsafe { ioctl::vidioc_try_encoder_cmd(fd.as_raw_fd(), &mut enc_cmd) } {
        Ok(_) => Ok(
            O::try_from(enc_cmd).map_err(|_| EncoderCmdError::FromV4L2CommandConversionError)?
        ),
        Err(e) => Err(e.into()),
    }
}

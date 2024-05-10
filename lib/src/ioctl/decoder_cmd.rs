use crate::bindings;
use crate::bindings::v4l2_decoder_cmd;
use bitflags::bitflags;
use nix::errno::Errno;
use std::convert::Infallible;
use std::convert::TryFrom;
use std::os::unix::io::AsRawFd;
use thiserror::Error;

#[doc(hidden)]
mod ioctl {
    use crate::bindings::v4l2_decoder_cmd;
    nix::ioctl_readwrite!(vidioc_decoder_cmd, b'V', 96, v4l2_decoder_cmd);
    nix::ioctl_readwrite!(vidioc_try_decoder_cmd, b'V', 97, v4l2_decoder_cmd);
}

bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct StartCmdFlags: u32 {
        const MUTE_AUDIO = bindings::V4L2_DEC_CMD_START_MUTE_AUDIO;
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct StopCmdFlags: u32 {
        const TO_BLACK = bindings::V4L2_DEC_CMD_STOP_TO_BLACK;
        const IMMEDIATELY = bindings::V4L2_DEC_CMD_STOP_IMMEDIATELY;
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct PauseCmdFlags: u32 {
        const TO_BLACK = bindings::V4L2_DEC_CMD_PAUSE_TO_BLACK;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, enumn::N)]
#[repr(u32)]
pub enum DecoderStartCmdFormat {
    #[default]
    None = bindings::V4L2_DEC_START_FMT_NONE,
    Gop = bindings::V4L2_DEC_START_FMT_GOP,
}

/// Safe variant of `struct v4l2_decoder_cmd`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecoderCmd {
    Start {
        flags: StartCmdFlags,
        speed: i32,
        format: DecoderStartCmdFormat,
    },
    Stop {
        flags: StopCmdFlags,
        pts: u64,
    },
    Pause {
        flags: PauseCmdFlags,
    },
    Resume,
}

impl DecoderCmd {
    /// Returns a simple START command without any extra parameter.
    pub fn start() -> Self {
        DecoderCmd::Start {
            flags: StartCmdFlags::empty(),
            speed: Default::default(),
            format: Default::default(),
        }
    }

    /// Returns a simple STOP command without any extra parameter.
    pub fn stop() -> Self {
        DecoderCmd::Stop {
            flags: StopCmdFlags::empty(),
            pts: Default::default(),
        }
    }

    /// Returns a simple PAUSE command without any extra parameter.
    pub fn pause() -> Self {
        DecoderCmd::Pause {
            flags: PauseCmdFlags::empty(),
        }
    }

    /// Returns a RESUME command.
    pub fn resume() -> Self {
        DecoderCmd::Resume
    }
}

impl From<DecoderCmd> for v4l2_decoder_cmd {
    fn from(command: DecoderCmd) -> Self {
        match command {
            DecoderCmd::Start {
                flags,
                speed,
                format,
            } => v4l2_decoder_cmd {
                cmd: bindings::V4L2_DEC_CMD_START,
                flags: flags.bits(),
                __bindgen_anon_1: bindings::v4l2_decoder_cmd__bindgen_ty_1 {
                    start: bindings::v4l2_decoder_cmd__bindgen_ty_1__bindgen_ty_2 {
                        speed,
                        format: format as u32,
                    },
                },
            },
            DecoderCmd::Stop { flags, pts } => v4l2_decoder_cmd {
                cmd: bindings::V4L2_DEC_CMD_STOP,
                flags: flags.bits(),
                __bindgen_anon_1: bindings::v4l2_decoder_cmd__bindgen_ty_1 {
                    stop: bindings::v4l2_decoder_cmd__bindgen_ty_1__bindgen_ty_1 { pts },
                },
            },
            DecoderCmd::Pause { flags } => v4l2_decoder_cmd {
                cmd: bindings::V4L2_DEC_CMD_PAUSE,
                flags: flags.bits(),
                __bindgen_anon_1: Default::default(),
            },
            DecoderCmd::Resume => v4l2_decoder_cmd {
                cmd: bindings::V4L2_DEC_CMD_RESUME,
                flags: Default::default(),
                __bindgen_anon_1: Default::default(),
            },
        }
    }
}

#[derive(Debug, Error)]
pub enum BuildDecoderCmdError {
    #[error("invalid command code {0}")]
    InvalidCommandCode(u32),
    #[error("invalid start command flags 0x{0:x}")]
    InvalidStartFlags(u32),
    #[error("invalid start command format {0}")]
    InvalidStartFormat(u32),
    #[error("invalid stop command flags 0x{0:x}")]
    InvalidStopFlags(u32),
    #[error("invalid pause command flags 0x{0:x}")]
    InvalidPauseFlags(u32),
}

impl TryFrom<v4l2_decoder_cmd> for DecoderCmd {
    type Error = BuildDecoderCmdError;

    fn try_from(cmd: v4l2_decoder_cmd) -> Result<Self, Self::Error> {
        let cmd = match cmd.cmd {
            bindings::V4L2_DEC_CMD_START => {
                // SAFETY: safe because we confirmed we are dealing with a START command.
                let params = unsafe { cmd.__bindgen_anon_1.start };
                DecoderCmd::Start {
                    flags: StartCmdFlags::from_bits(cmd.flags)
                        .ok_or(BuildDecoderCmdError::InvalidStartFlags(cmd.flags))?,
                    speed: params.speed,
                    format: DecoderStartCmdFormat::n(params.format)
                        .ok_or(BuildDecoderCmdError::InvalidStartFormat(params.format))?,
                }
            }
            bindings::V4L2_DEC_CMD_STOP => DecoderCmd::Stop {
                flags: StopCmdFlags::from_bits(cmd.flags)
                    .ok_or(BuildDecoderCmdError::InvalidStopFlags(cmd.flags))?,
                // SAFETY: safe because we confirmed we are dealing with a STOP command.
                pts: unsafe { cmd.__bindgen_anon_1.stop.pts },
            },
            bindings::V4L2_DEC_CMD_PAUSE => DecoderCmd::Pause {
                flags: PauseCmdFlags::from_bits(cmd.flags)
                    .ok_or(BuildDecoderCmdError::InvalidPauseFlags(cmd.flags))?,
            },
            bindings::V4L2_DEC_CMD_RESUME => DecoderCmd::Resume,
            code => return Err(BuildDecoderCmdError::InvalidCommandCode(code)),
        };

        Ok(cmd)
    }
}

impl TryFrom<v4l2_decoder_cmd> for () {
    type Error = Infallible;

    fn try_from(_: v4l2_decoder_cmd) -> Result<Self, Self::Error> {
        Ok(())
    }
}

#[derive(Debug, Error)]
pub enum DecoderCmdError {
    #[error("error while converting from v4l2_decoder_cmd")]
    FromV4L2CommandConversionError,
    #[error("drain already in progress")]
    DrainInProgress,
    #[error("command not supported by device")]
    UnsupportedCommand,
    #[error("ioctl error: {0}")]
    IoctlError(Errno),
}

impl From<DecoderCmdError> for Errno {
    fn from(err: DecoderCmdError) -> Self {
        match err {
            DecoderCmdError::FromV4L2CommandConversionError => Errno::EINVAL,
            DecoderCmdError::DrainInProgress => Errno::EBUSY,
            DecoderCmdError::UnsupportedCommand => Errno::EINVAL,
            DecoderCmdError::IoctlError(e) => e,
        }
    }
}

impl From<Errno> for DecoderCmdError {
    fn from(error: Errno) -> Self {
        match error {
            Errno::EBUSY => DecoderCmdError::DrainInProgress,
            Errno::EINVAL => DecoderCmdError::UnsupportedCommand,
            e => DecoderCmdError::IoctlError(e),
        }
    }
}

/// Safe wrapper around the `VIDIOC_DECODER_CMD` ioctl.
pub fn decoder_cmd<I: Into<v4l2_decoder_cmd>, O: TryFrom<v4l2_decoder_cmd>>(
    fd: &impl AsRawFd,
    command: I,
) -> Result<O, DecoderCmdError> {
    let mut dec_cmd = command.into();

    match unsafe { ioctl::vidioc_decoder_cmd(fd.as_raw_fd(), &mut dec_cmd) } {
        Ok(_) => Ok(
            O::try_from(dec_cmd).map_err(|_| DecoderCmdError::FromV4L2CommandConversionError)?
        ),
        Err(e) => Err(e.into()),
    }
}

/// Safe wrapper around the `VIDIOC_TRY_DECODER_CMD` ioctl.
pub fn try_decoder_cmd<I: Into<v4l2_decoder_cmd>, O: TryFrom<v4l2_decoder_cmd>>(
    fd: &impl AsRawFd,
    command: I,
) -> Result<O, DecoderCmdError> {
    let mut dec_cmd = command.into();

    match unsafe { ioctl::vidioc_try_decoder_cmd(fd.as_raw_fd(), &mut dec_cmd) } {
        Ok(_) => Ok(
            O::try_from(dec_cmd).map_err(|_| DecoderCmdError::FromV4L2CommandConversionError)?
        ),
        Err(e) => Err(e.into()),
    }
}

#[cfg(test)]
mod tests {
    use std::convert::TryFrom;

    use crate::{
        bindings,
        ioctl::{DecoderStartCmdFormat, PauseCmdFlags, StartCmdFlags, StopCmdFlags},
    };

    use super::DecoderCmd;

    #[test]
    fn build_decoder_cmd() {
        // Build START command and back.
        let cmd = bindings::v4l2_decoder_cmd {
            cmd: bindings::V4L2_DEC_CMD_START,
            flags: bindings::V4L2_DEC_CMD_START_MUTE_AUDIO,
            __bindgen_anon_1: bindings::v4l2_decoder_cmd__bindgen_ty_1 {
                start: bindings::v4l2_decoder_cmd__bindgen_ty_1__bindgen_ty_2 {
                    speed: 42,
                    format: bindings::V4L2_DEC_START_FMT_GOP,
                },
            },
        };
        let cmd_safe = DecoderCmd::try_from(cmd).unwrap();
        assert_eq!(
            cmd_safe,
            DecoderCmd::Start {
                flags: StartCmdFlags::MUTE_AUDIO,
                speed: 42,
                format: DecoderStartCmdFormat::Gop,
            }
        );
        let cmd_rebuilt: bindings::v4l2_decoder_cmd = cmd_safe.into();
        assert_eq!(cmd_safe, DecoderCmd::try_from(cmd_rebuilt).unwrap());

        // Build STOP command and back.
        let cmd = bindings::v4l2_decoder_cmd {
            cmd: bindings::V4L2_DEC_CMD_STOP,
            flags: bindings::V4L2_DEC_CMD_STOP_IMMEDIATELY | bindings::V4L2_DEC_CMD_STOP_TO_BLACK,
            __bindgen_anon_1: bindings::v4l2_decoder_cmd__bindgen_ty_1 {
                stop: bindings::v4l2_decoder_cmd__bindgen_ty_1__bindgen_ty_1 { pts: 496000 },
            },
        };
        let cmd_safe = DecoderCmd::try_from(cmd).unwrap();
        assert_eq!(
            cmd_safe,
            DecoderCmd::Stop {
                flags: StopCmdFlags::IMMEDIATELY | StopCmdFlags::TO_BLACK,
                pts: 496000,
            }
        );
        let cmd_rebuilt: bindings::v4l2_decoder_cmd = cmd_safe.into();
        assert_eq!(cmd_safe, DecoderCmd::try_from(cmd_rebuilt).unwrap());

        // Build PAUSE command and back.
        let cmd = bindings::v4l2_decoder_cmd {
            cmd: bindings::V4L2_DEC_CMD_PAUSE,
            flags: bindings::V4L2_DEC_CMD_PAUSE_TO_BLACK,
            __bindgen_anon_1: Default::default(),
        };
        let cmd_safe = DecoderCmd::try_from(cmd).unwrap();
        assert_eq!(
            cmd_safe,
            DecoderCmd::Pause {
                flags: PauseCmdFlags::TO_BLACK,
            }
        );
        let cmd_rebuilt: bindings::v4l2_decoder_cmd = cmd_safe.into();
        assert_eq!(cmd_safe, DecoderCmd::try_from(cmd_rebuilt).unwrap());

        // Build RESUME command and back.
        let cmd = bindings::v4l2_decoder_cmd {
            cmd: bindings::V4L2_DEC_CMD_RESUME,
            flags: Default::default(),
            __bindgen_anon_1: Default::default(),
        };
        let cmd_safe = DecoderCmd::try_from(cmd).unwrap();
        assert_eq!(cmd_safe, DecoderCmd::Resume);
        let cmd_rebuilt: bindings::v4l2_decoder_cmd = cmd_safe.into();
        assert_eq!(cmd_safe, DecoderCmd::try_from(cmd_rebuilt).unwrap());
    }
}

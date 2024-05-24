//! Safe wrapper for the `VIDIOC_(TRY_)DECODER_CMD` ioctls.

use bitflags::bitflags;
use nix::errno::Errno;
use std::convert::Infallible;
use std::convert::TryFrom;
use std::os::unix::io::AsRawFd;
use thiserror::Error;

use crate::bindings;
use crate::bindings::v4l2_decoder_cmd;
use crate::ioctl::ioctl_and_convert;
use crate::ioctl::IoctlConvertError;
use crate::ioctl::IoctlConvertResult;

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
                    flags: StartCmdFlags::from_bits_truncate(cmd.flags),
                    speed: params.speed,
                    format: DecoderStartCmdFormat::n(params.format).unwrap_or_default(),
                }
            }
            bindings::V4L2_DEC_CMD_STOP => DecoderCmd::Stop {
                flags: StopCmdFlags::from_bits_truncate(cmd.flags),
                // SAFETY: safe because we confirmed we are dealing with a STOP command.
                pts: unsafe { cmd.__bindgen_anon_1.stop.pts },
            },
            bindings::V4L2_DEC_CMD_PAUSE => DecoderCmd::Pause {
                flags: PauseCmdFlags::from_bits_truncate(cmd.flags),
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
pub enum DecoderCmdIoctlError {
    #[error("drain already in progress")]
    DrainInProgress,
    #[error("command not supported by device")]
    UnsupportedCommand,
    #[error("unexpected ioctl error: {0}")]
    Other(Errno),
}

impl From<DecoderCmdIoctlError> for Errno {
    fn from(err: DecoderCmdIoctlError) -> Self {
        match err {
            DecoderCmdIoctlError::DrainInProgress => Errno::EBUSY,
            DecoderCmdIoctlError::UnsupportedCommand => Errno::EINVAL,
            DecoderCmdIoctlError::Other(e) => e,
        }
    }
}

impl From<Errno> for DecoderCmdIoctlError {
    fn from(error: Errno) -> Self {
        match error {
            Errno::EBUSY => DecoderCmdIoctlError::DrainInProgress,
            Errno::EINVAL => DecoderCmdIoctlError::UnsupportedCommand,
            e => DecoderCmdIoctlError::Other(e),
        }
    }
}

#[doc(hidden)]
mod ioctl {
    use crate::bindings::v4l2_decoder_cmd;
    nix::ioctl_readwrite!(vidioc_decoder_cmd, b'V', 96, v4l2_decoder_cmd);
    nix::ioctl_readwrite!(vidioc_try_decoder_cmd, b'V', 97, v4l2_decoder_cmd);
}

pub type DecoderCmdError<CE> = IoctlConvertError<DecoderCmdIoctlError, CE>;
pub type DecoderCmdResult<O, CE> = IoctlConvertResult<O, DecoderCmdIoctlError, CE>;

/// Safe wrapper around the `VIDIOC_DECODER_CMD` ioctl.
pub fn decoder_cmd<I, O>(fd: &impl AsRawFd, command: I) -> DecoderCmdResult<O, O::Error>
where
    I: Into<v4l2_decoder_cmd>,
    O: TryFrom<v4l2_decoder_cmd>,
    O::Error: std::fmt::Debug,
{
    let mut dec_cmd = command.into();

    ioctl_and_convert(
        unsafe { ioctl::vidioc_decoder_cmd(fd.as_raw_fd(), &mut dec_cmd) }
            .map(|_| dec_cmd)
            .map_err(Into::into),
    )
}

/// Safe wrapper around the `VIDIOC_TRY_DECODER_CMD` ioctl.
pub fn try_decoder_cmd<I, O>(fd: &impl AsRawFd, command: I) -> DecoderCmdResult<O, O::Error>
where
    I: Into<v4l2_decoder_cmd>,
    O: TryFrom<v4l2_decoder_cmd>,
    O::Error: std::fmt::Debug,
{
    let mut dec_cmd = command.into();

    ioctl_and_convert(
        unsafe { ioctl::vidioc_try_decoder_cmd(fd.as_raw_fd(), &mut dec_cmd) }
            .map(|_| dec_cmd)
            .map_err(Into::into),
    )
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

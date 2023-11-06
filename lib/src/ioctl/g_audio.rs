use std::mem;
use std::os::unix::io::AsRawFd;

use bitflags::bitflags;
use enumn::N;
use nix::errno::Errno;
use thiserror::Error;

use crate::bindings;
use crate::bindings::v4l2_audio;
use crate::bindings::v4l2_audioout;
use crate::bindings::v4l2_frequency;
use crate::bindings::v4l2_modulator;
use crate::bindings::v4l2_tuner;

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

#[derive(Debug, N)]
#[repr(u32)]
pub enum TunerType {
    Radio = bindings::v4l2_tuner_type_V4L2_TUNER_RADIO,
    AnalogTv = bindings::v4l2_tuner_type_V4L2_TUNER_ANALOG_TV,
    DigitalTv = bindings::v4l2_tuner_type_V4L2_TUNER_DIGITAL_TV,
    Sdr = bindings::v4l2_tuner_type_V4L2_TUNER_SDR,
    Rf = bindings::v4l2_tuner_type_V4L2_TUNER_RF,
}

bitflags! {
    #[derive(Clone, Copy, Debug)]
    pub struct TunerCapFlags: u32 {
        const LOW = bindings::V4L2_TUNER_CAP_LOW;
        const NORM = bindings::V4L2_TUNER_CAP_NORM;
        const HWSEEK_BOUNDED = bindings::V4L2_TUNER_CAP_HWSEEK_BOUNDED;
        const HWSEEK_WRAP = bindings::V4L2_TUNER_CAP_HWSEEK_WRAP;
        const STEREO = bindings::V4L2_TUNER_CAP_STEREO;
        const LANG1 = bindings::V4L2_TUNER_CAP_LANG1;
        const LANG2 = bindings::V4L2_TUNER_CAP_LANG2;
        const SAP = bindings::V4L2_TUNER_CAP_SAP;
        const RDS = bindings::V4L2_TUNER_CAP_RDS;
        const RDS_BLOCK_IO = bindings::V4L2_TUNER_CAP_RDS_BLOCK_IO;
        const RDS_CONTROLS = bindings::V4L2_TUNER_CAP_RDS_CONTROLS;
        const FREQ_BANDS = bindings::V4L2_TUNER_CAP_FREQ_BANDS;
        const HWSEEK_PROG_LIM = bindings::V4L2_TUNER_CAP_HWSEEK_PROG_LIM;
        const ONE_HZ = bindings::V4L2_TUNER_CAP_1HZ;
    }

    #[derive(Clone, Copy, Debug)]
    pub struct TunerTransmissionFlags: u32 {
        const MONO = bindings::V4L2_TUNER_SUB_MONO;
        const STEREO = bindings::V4L2_TUNER_SUB_STEREO;
        const LANG1 = bindings::V4L2_TUNER_SUB_LANG1;
        const LANG2 = bindings::V4L2_TUNER_SUB_LANG2;
        const RDS = bindings::V4L2_TUNER_SUB_RDS;
    }
}

#[derive(Debug, N)]
#[repr(u32)]
pub enum TunerMode {
    Mono = bindings::V4L2_TUNER_MODE_MONO,
    Stereo = bindings::V4L2_TUNER_MODE_STEREO,
    Lang1 = bindings::V4L2_TUNER_MODE_LANG1,
    Lang2 = bindings::V4L2_TUNER_MODE_LANG2,
    Lang1Lang2 = bindings::V4L2_TUNER_MODE_LANG1_LANG2,
}

#[doc(hidden)]
mod ioctl {
    use crate::bindings::v4l2_audio;
    use crate::bindings::v4l2_audioout;
    use crate::bindings::v4l2_frequency;
    use crate::bindings::v4l2_modulator;
    use crate::bindings::v4l2_tuner;

    nix::ioctl_readwrite!(vidioc_g_tuner, b'V', 29, v4l2_tuner);
    nix::ioctl_write_ptr!(vidioc_s_tuner, b'V', 30, v4l2_tuner);

    nix::ioctl_read!(vidioc_g_audio, b'V', 33, v4l2_audio);
    nix::ioctl_write_ptr!(vidioc_s_audio, b'V', 34, v4l2_audio);

    nix::ioctl_read!(vidioc_g_audout, b'V', 49, v4l2_audioout);
    nix::ioctl_write_ptr!(vidioc_s_audout, b'V', 50, v4l2_audioout);

    nix::ioctl_readwrite!(vidioc_g_modulator, b'V', 54, v4l2_modulator);
    nix::ioctl_write_ptr!(vidioc_s_modulator, b'V', 55, v4l2_modulator);

    nix::ioctl_readwrite!(vidioc_g_frequency, b'V', 56, v4l2_frequency);
    nix::ioctl_write_ptr!(vidioc_s_frequency, b'V', 57, v4l2_frequency);

    nix::ioctl_readwrite!(vidioc_enumaudio, b'V', 65, v4l2_audio);
    nix::ioctl_readwrite!(vidioc_enumaudout, b'V', 66, v4l2_audioout);
}

#[derive(Debug, Error)]
pub enum GAudioError {
    #[error("invalid input index")]
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

/// Safe wrapper around the `VIDIOC_G_TUNER` ioctl.
pub fn g_tuner<O: From<v4l2_tuner>>(fd: &impl AsRawFd, index: u32) -> Result<O, GAudioError> {
    let mut tuner = v4l2_tuner {
        index,
        ..unsafe { mem::zeroed() }
    };

    match unsafe { ioctl::vidioc_g_tuner(fd.as_raw_fd(), &mut tuner) } {
        Ok(_) => Ok(O::from(tuner)),
        Err(Errno::EINVAL) => Err(GAudioError::Invalid),
        Err(e) => Err(GAudioError::IoctlError(e)),
    }
}

/// Safe wrapper around the `VIDIOC_S_TUNER` ioctl.
pub fn s_tuner(fd: &impl AsRawFd, index: u32, mode: TunerMode) -> Result<(), GAudioError> {
    let tuner = v4l2_tuner {
        index,
        audmode: mode as u32,
        ..unsafe { mem::zeroed() }
    };

    match unsafe { ioctl::vidioc_s_tuner(fd.as_raw_fd(), &tuner) } {
        Ok(_) => Ok(()),
        Err(Errno::EINVAL) => Err(GAudioError::Invalid),
        Err(e) => Err(GAudioError::IoctlError(e)),
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
pub fn s_audio(fd: &impl AsRawFd, index: u32, mode: Option<AudioMode>) -> Result<(), GAudioError> {
    let audio = v4l2_audio {
        index,
        mode: mode.map(|m| m as u32).unwrap_or(0),
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

/// Safe wrapper around the `VIDIOC_G_MODULATOR` ioctl.
pub fn g_modulator<O: From<v4l2_modulator>>(
    fd: &impl AsRawFd,
    index: u32,
) -> Result<O, GAudioError> {
    let mut modulator = v4l2_modulator {
        index,
        ..unsafe { mem::zeroed() }
    };

    match unsafe { ioctl::vidioc_g_modulator(fd.as_raw_fd(), &mut modulator) } {
        Ok(_) => Ok(O::from(modulator)),
        Err(Errno::EINVAL) => Err(GAudioError::Invalid),
        Err(e) => Err(GAudioError::IoctlError(e)),
    }
}

/// Safe wrapper around the `VIDIOC_S_MODULATOR` ioctl.
pub fn s_modulator(
    fd: &impl AsRawFd,
    index: u32,
    txsubchans: TunerTransmissionFlags,
) -> Result<(), GAudioError> {
    let modulator = v4l2_modulator {
        index,
        txsubchans: txsubchans.bits(),
        ..unsafe { mem::zeroed() }
    };

    match unsafe { ioctl::vidioc_s_modulator(fd.as_raw_fd(), &modulator) } {
        Ok(_) => Ok(()),
        Err(Errno::EINVAL) => Err(GAudioError::Invalid),
        Err(e) => Err(GAudioError::IoctlError(e)),
    }
}

/// Safe wrapper around the `VIDIOC_G_FREQUENCY` ioctl.
pub fn g_frequency<O: From<v4l2_frequency>>(
    fd: &impl AsRawFd,
    tuner: u32,
) -> Result<O, GAudioError> {
    let mut frequency = v4l2_frequency {
        tuner,
        ..unsafe { mem::zeroed() }
    };

    match unsafe { ioctl::vidioc_g_frequency(fd.as_raw_fd(), &mut frequency) } {
        Ok(_) => Ok(O::from(frequency)),
        Err(Errno::EINVAL) => Err(GAudioError::Invalid),
        Err(e) => Err(GAudioError::IoctlError(e)),
    }
}

/// Safe wrapper around the `VIDIOC_S_FREQUENCY` ioctl.
pub fn s_frequency(
    fd: &impl AsRawFd,
    tuner: u32,
    type_: TunerType,
    frequency: u32,
) -> Result<(), GAudioError> {
    let frequency = v4l2_frequency {
        tuner,
        type_: type_ as u32,
        frequency,
        ..unsafe { mem::zeroed() }
    };

    match unsafe { ioctl::vidioc_s_frequency(fd.as_raw_fd(), &frequency) } {
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

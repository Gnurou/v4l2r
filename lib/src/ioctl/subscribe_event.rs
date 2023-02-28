//! Safe wrapper for the `VIDIOC_SUBSCRIBE_EVENT` and `VIDIOC_UNSUBSCRIBE_EVENT
//! ioctls.

use nix::errno::Errno;
use std::os::unix::io::AsRawFd;
use std::{
    convert::{TryFrom, TryInto},
    mem,
};
use thiserror::Error;

use crate::bindings;
use bitflags::bitflags;

bitflags! {
    pub struct SubscribeEventFlags: u32 {
        const SEND_INITIAL = bindings::V4L2_EVENT_SUB_FL_SEND_INITIAL;
        const ALLOW_FEEDBACK = bindings::V4L2_EVENT_SUB_FL_ALLOW_FEEDBACK;
    }

}

pub enum EventType {
    VSync,
    Eos,
    Ctrl(u32),
    FrameSync,
    SourceChange,
    MotionDet,
}

bitflags! {
    pub struct SrcChanges: u32 {
        const RESOLUTION = bindings::V4L2_EVENT_SRC_CH_RESOLUTION;
    }
}

#[derive(Debug)]
pub enum Event {
    SrcChangeEvent(SrcChanges),
}

#[derive(Debug, Error)]
pub enum EventConversionError {
    #[error("unrecognized event {0}")]
    UnrecognizedEvent(u32),
    #[error("unrecognized source change {0}")]
    UnrecognizedSourceChange(u32),
}

impl TryFrom<bindings::v4l2_event> for Event {
    type Error = EventConversionError;

    fn try_from(value: bindings::v4l2_event) -> Result<Self, Self::Error> {
        Ok(match value.type_ {
            bindings::V4L2_EVENT_VSYNC => todo!(),
            bindings::V4L2_EVENT_EOS => todo!(),
            bindings::V4L2_EVENT_CTRL => todo!(),
            bindings::V4L2_EVENT_FRAME_SYNC => todo!(),
            bindings::V4L2_EVENT_SOURCE_CHANGE => {
                let changes = unsafe { value.u.src_change.changes };
                Event::SrcChangeEvent(
                    SrcChanges::from_bits(changes)
                        .ok_or(EventConversionError::UnrecognizedSourceChange(changes))?,
                )
            }
            bindings::V4L2_EVENT_MOTION_DET => todo!(),
            t => return Err(EventConversionError::UnrecognizedEvent(t)),
        })
    }
}

fn build_v4l2_event_subscription(
    event: EventType,
    flags: SubscribeEventFlags,
) -> bindings::v4l2_event_subscription {
    bindings::v4l2_event_subscription {
        type_: match event {
            EventType::VSync => bindings::V4L2_EVENT_VSYNC,
            EventType::Eos => bindings::V4L2_EVENT_EOS,
            EventType::Ctrl(_) => bindings::V4L2_EVENT_CTRL,
            EventType::FrameSync => bindings::V4L2_EVENT_FRAME_SYNC,
            EventType::SourceChange => bindings::V4L2_EVENT_SOURCE_CHANGE,
            EventType::MotionDet => bindings::V4L2_EVENT_MOTION_DET,
        },
        id: match event {
            EventType::Ctrl(id) => id,
            _ => 0,
        },
        flags: flags.bits(),
        ..unsafe { mem::zeroed() }
    }
}

#[doc(hidden)]
mod ioctl {
    use crate::bindings::{v4l2_event, v4l2_event_subscription};

    nix::ioctl_read!(vidioc_dqevent, b'V', 89, v4l2_event);
    nix::ioctl_write_ptr!(vidioc_subscribe_event, b'V', 90, v4l2_event_subscription);
    nix::ioctl_write_ptr!(vidioc_unsubscribe_event, b'V', 91, v4l2_event_subscription);
}

#[derive(Debug, Error)]
pub enum SubscribeEventError {
    #[error("ioctl error: {0}")]
    IoctlError(#[from] Errno),
}

impl From<SubscribeEventError> for Errno {
    fn from(err: SubscribeEventError) -> Self {
        match err {
            SubscribeEventError::IoctlError(e) => e,
        }
    }
}

pub fn subscribe_event(
    fd: &impl AsRawFd,
    event: EventType,
    flags: SubscribeEventFlags,
) -> Result<(), SubscribeEventError> {
    let subscription = build_v4l2_event_subscription(event, flags);

    unsafe { ioctl::vidioc_subscribe_event(fd.as_raw_fd(), &subscription) }?;
    Ok(())
}

pub fn unsubscribe_event(fd: &impl AsRawFd, event: EventType) -> Result<(), SubscribeEventError> {
    let subscription = build_v4l2_event_subscription(event, SubscribeEventFlags::empty());

    unsafe { ioctl::vidioc_unsubscribe_event(fd.as_raw_fd(), &subscription) }?;
    Ok(())
}

pub fn unsubscribe_all_events(fd: &impl AsRawFd) -> Result<(), SubscribeEventError> {
    let subscription = bindings::v4l2_event_subscription {
        type_: bindings::V4L2_EVENT_ALL,
        ..unsafe { mem::zeroed() }
    };

    unsafe { ioctl::vidioc_unsubscribe_event(fd.as_raw_fd(), &subscription) }?;
    Ok(())
}

#[derive(Debug, Error)]
pub enum DqEventError {
    #[error("no event ready for dequeue")]
    NotReady,
    #[error("error while converting event")]
    EventConversionError(#[from] EventConversionError),
    #[error("ioctl error: {0}")]
    IoctlError(Errno),
}

impl From<Errno> for DqEventError {
    fn from(error: Errno) -> Self {
        match error {
            Errno::ENOENT => Self::NotReady,
            error => Self::IoctlError(error),
        }
    }
}

impl From<DqEventError> for Errno {
    fn from(err: DqEventError) -> Self {
        match err {
            DqEventError::NotReady => Errno::ENOENT,
            DqEventError::EventConversionError(_) => Errno::EINVAL,
            DqEventError::IoctlError(e) => e,
        }
    }
}

pub fn dqevent(fd: &impl AsRawFd) -> Result<Event, DqEventError> {
    // Safe because this struct is expected to be initialized to 0.
    let mut event: bindings::v4l2_event = unsafe { mem::zeroed() };
    unsafe { ioctl::vidioc_dqevent(fd.as_raw_fd(), &mut event) }?;

    Ok(event.try_into()?)
}

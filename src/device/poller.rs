//! A level-triggered `Poller` for V4L2 devices that allows a user to be notified
//! when a CAPTURE or OUTPUT buffer is ready to be dequeued, or when a V4L2
//! event is ready to be dequeued.
//!
//! It also provides a `Waker` companion that allows other threads to interrupt
//! an ongoing (or coming) poll. Useful to implement an event-based loop.

use bitflags::bitflags;
use std::{
    fs::File,
    io::{self, Read, Write},
    mem,
    os::unix::io::{AsRawFd, FromRawFd},
    sync::atomic::AtomicUsize,
    sync::atomic::Ordering,
    sync::Arc,
};

use crate::device::Device;

macro_rules! syscall {
    ($f: ident ( $($args: expr),* $(,)* ) ) => {{
        match unsafe { libc::$f($($args, )*) } {
            err if err < 0 => Err(std::io::Error::last_os_error()),
            res => Ok(res)
        }
    }};
}

pub enum DeviceEvent {
    CaptureReady,
    OutputReady,
    V4L2Event,
}

bitflags! {
    /// A set of polling events returned by `poll()`. It can contain several
    /// events which can be iterated over for convenience.
    pub struct PollEvents: u32 {
        const DEVICE_CAPTURE = 0b001;
        const DEVICE_OUTPUT = 0b010;
        const DEVICE_EVENT = 0b100;
        const WAKER = 0b1000;
    }
}

impl Iterator for PollEvents {
    type Item = PollEvents;

    fn next(&mut self) -> Option<Self::Item> {
        if self.is_empty() {
            return None;
        }

        let first_set = self.bits.trailing_zeros();
        // Safe because we extracted the bit from a valid value.
        let next = unsafe { PollEvents::from_bits_unchecked(1 << first_set) };
        self.remove(next);
        Some(next)
    }
}

#[cfg(test)]
mod tests {
    use super::PollEvents;

    #[test]
    fn test_pollevents_iterator() {
        let mut poll_events = PollEvents::empty();
        assert_eq!(poll_events.next(), None);

        let mut poll_events = PollEvents::DEVICE_CAPTURE;
        assert_eq!(poll_events.next(), Some(PollEvents::DEVICE_CAPTURE));
        assert_eq!(poll_events.next(), None);

        let mut poll_events = PollEvents::DEVICE_EVENT;
        assert_eq!(poll_events.next(), Some(PollEvents::DEVICE_EVENT));
        assert_eq!(poll_events.next(), None);

        let mut poll_events = PollEvents::DEVICE_OUTPUT | PollEvents::WAKER;
        assert_eq!(poll_events.next(), Some(PollEvents::DEVICE_OUTPUT));
        assert_eq!(poll_events.next(), Some(PollEvents::WAKER));
        assert_eq!(poll_events.next(), None);

        let mut poll_events = PollEvents::all();
        assert_eq!(poll_events.next(), Some(PollEvents::DEVICE_CAPTURE));
        assert_eq!(poll_events.next(), Some(PollEvents::DEVICE_OUTPUT));
        assert_eq!(poll_events.next(), Some(PollEvents::DEVICE_EVENT));
        assert_eq!(poll_events.next(), Some(PollEvents::WAKER));
        assert_eq!(poll_events.next(), None);
    }
}

pub struct Waker {
    fd: File,
}

impl Waker {
    fn new() -> io::Result<Self> {
        let fd = syscall!(eventfd(0, libc::EFD_CLOEXEC | libc::EFD_NONBLOCK))?;

        Ok(Waker {
            fd: unsafe { File::from_raw_fd(fd) },
        })
    }

    pub fn wake(&self) -> io::Result<()> {
        let buf = 1u64.to_ne_bytes();
        // Files support concurrent access at the OS level. The implementation
        // of Write for &File lets us call the write mutable method even on a
        // non-mutable File instance.
        (&self.fd).write(&buf).map(|_| ())
    }

    /// Perform a read on this waker in order to reset its counter to 0. This
    /// means it will make subsequent calls to `poll()` block until `wake()` is
    /// called again.
    fn reset(&self) -> io::Result<()> {
        let mut buf = 0u64.to_ne_bytes();
        (&self.fd).read(&mut buf).map(|_| ())
    }
}

pub struct Poller {
    device: Arc<Device>,
    waker: Arc<Waker>,
    epoll: File,

    // Whether or not to listen to specific device events.
    capture_enabled: bool,
    output_enabled: bool,
    events_enabled: bool,

    // If set, incremented every time we wake up from a poll.
    poll_wakeups_counter: Option<Arc<AtomicUsize>>,
}

const DEVICE_ID: u64 = 1;
const WAKER_ID: u64 = 2;

impl Poller {
    pub fn new(device: Arc<Device>) -> io::Result<Self> {
        let epoll = syscall!(epoll_create1(libc::EFD_CLOEXEC))
            .map(|fd| unsafe { File::from_raw_fd(fd) })?;
        let waker = Waker::new()?;

        // Register our device.
        // There is a bug in some Linux kernels (at least 5.9 and older) where EPOLLIN
        // and EPOLLOUT events wont be signaled to epoll if the first call to epoll did
        // not include at least one of EPOLLIN or EPOLLOUT as desired events.
        // Make sure we don't fall into this trap by registering EPOLLIN first and doing
        // a dummy poll call. This call will immediately return with an error because the
        // CAPTURE queue is not streaming, but it will set the right hooks in the kernel
        // and we can now reconfigure our events to only include EPOLLPRI and have poll
        // working as expected.
        syscall!(epoll_ctl(
            epoll.as_raw_fd(),
            libc::EPOLL_CTL_ADD,
            device.as_raw_fd(),
            &mut libc::epoll_event {
                events: libc::EPOLLIN as u32,
                u64: DEVICE_ID,
            }
        ))?;
        // This call should return an EPOLLERR event immediately. But it will
        // also ensure that the CAPTURE and OUTPUT poll handlers are registered
        // in the kernel for our device.
        syscall!(epoll_wait(
            epoll.as_raw_fd(),
            &mut libc::epoll_event { ..mem::zeroed() },
            1,
            -1
        ))?;
        // Now reset our device events.
        syscall!(epoll_ctl(
            epoll.as_raw_fd(),
            libc::EPOLL_CTL_MOD,
            device.as_raw_fd(),
            &mut libc::epoll_event {
                events: 0u32,
                u64: DEVICE_ID,
            }
        ))?;

        // Register the waker
        syscall!(epoll_ctl(
            epoll.as_raw_fd(),
            libc::EPOLL_CTL_ADD,
            waker.fd.as_raw_fd(),
            &mut libc::epoll_event {
                events: libc::EPOLLIN as u32,
                u64: WAKER_ID,
            }
        ))?;

        Ok(Poller {
            device,
            waker: Arc::new(waker),
            epoll,
            capture_enabled: false,
            output_enabled: false,
            events_enabled: false,
            poll_wakeups_counter: None,
        })
    }

    pub fn get_waker(&self) -> &Arc<Waker> {
        &self.waker
    }

    pub fn set_poll_counter(&mut self, poll_wakeup_counter: Arc<AtomicUsize>) {
        self.poll_wakeups_counter = Some(poll_wakeup_counter);
    }

    fn update_device_registration(&mut self) -> io::Result<()> {
        let mut epoll_event = libc::epoll_event {
            events: 0u32,
            u64: DEVICE_ID,
        };

        if self.capture_enabled {
            epoll_event.events |= libc::EPOLLIN as u32;
        }
        if self.output_enabled {
            epoll_event.events |= libc::EPOLLOUT as u32;
        }
        if self.events_enabled {
            epoll_event.events |= libc::EPOLLPRI as u32;
        }

        syscall!(epoll_ctl(
            self.epoll.as_raw_fd(),
            libc::EPOLL_CTL_MOD,
            self.device.as_raw_fd(),
            &mut epoll_event
        ))
        .map(|_| ())
    }

    fn set_event(&mut self, event: DeviceEvent, enable: bool) -> io::Result<()> {
        let event = match event {
            DeviceEvent::CaptureReady => &mut self.capture_enabled,
            DeviceEvent::OutputReady => &mut self.output_enabled,
            DeviceEvent::V4L2Event => &mut self.events_enabled,
        };

        // Do not alter event if it was already in the desired state.
        if *event == enable {
            return Ok(());
        }

        *event = enable;
        self.update_device_registration()
    }

    /// Enable listening to (and reporting) `event`.
    pub fn enable_event(&mut self, event: DeviceEvent) -> io::Result<()> {
        self.set_event(event, true)
    }

    /// Disable listening to (and reporting of) `event`.
    pub fn disable_event(&mut self, event: DeviceEvent) -> io::Result<()> {
        self.set_event(event, false)
    }

    /// Returns whether the given event is currently listened to.
    pub fn is_event_enabled(&self, event: DeviceEvent) -> bool {
        match event {
            DeviceEvent::CaptureReady => self.capture_enabled,
            DeviceEvent::OutputReady => self.output_enabled,
            DeviceEvent::V4L2Event => self.events_enabled,
        }
    }

    pub fn poll(&mut self, duration: Option<std::time::Duration>) -> io::Result<PollEvents> {
        let mut events: [libc::epoll_event; 4] = unsafe { mem::zeroed() };
        let duration: i32 = match duration {
            None => -1,
            Some(d) => d.as_millis() as i32,
        };

        let nb_events = syscall!(epoll_wait(
            self.epoll.as_raw_fd(),
            events.as_mut_ptr(),
            events.len() as i32,
            duration
        ))? as usize;

        // Update our wake up stats
        if let Some(wakeup_counter) = &self.poll_wakeups_counter {
            wakeup_counter.fetch_add(1, Ordering::SeqCst);
        }

        // Check which events we got.
        let mut ret = PollEvents::empty();
        for event in &events[0..nb_events] {
            match event.u64 {
                DEVICE_ID => {
                    let events = event.events as i32;
                    if events & libc::EPOLLIN != 0 {
                        ret |= PollEvents::DEVICE_CAPTURE;
                    }
                    if events & libc::EPOLLOUT != 0 {
                        ret |= PollEvents::DEVICE_OUTPUT;
                    }
                    if events & libc::EPOLLPRI != 0 {
                        ret |= PollEvents::DEVICE_EVENT;
                    }
                }
                WAKER_ID => {
                    ret |= PollEvents::WAKER;
                    self.waker.reset()?;
                }
                _ => panic!("Unregistered token returned by epoll_wait!"),
            }
        }

        Ok(ret)
    }
}

//! A level-triggered `Poller` for V4L2 devices that allows a user to be notified
//! when a CAPTURE or OUTPUT buffer is ready to be dequeued, or when a V4L2
//! event is ready to be dequeued.
//!
//! It also provides a `Waker` companion that allows other threads to interrupt
//! an ongoing (or coming) poll. Useful to implement an event-based loop.

use std::{
    collections::BTreeMap,
    fs::File,
    io::{self, Read, Write},
    os::fd::{AsFd, BorrowedFd},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    task::Wake,
};

use log::{error, warn};
use nix::sys::{
    epoll::{Epoll, EpollCreateFlags, EpollEvent, EpollFlags},
    eventfd::{eventfd, EfdFlags},
};
use thiserror::Error;

use crate::device::Device;

#[derive(Debug, PartialEq)]
pub enum DeviceEvent {
    CaptureReady,
    OutputReady,
    V4L2Event,
}

#[derive(Debug, PartialEq)]
pub enum PollEvent {
    Device(DeviceEvent),
    Waker(u32),
}

pub struct PollEvents {
    events: [EpollEvent; 4],
    nb_events: usize,
    cur_event: usize,
}

impl PollEvents {
    fn new() -> Self {
        PollEvents {
            events: [
                EpollEvent::empty(),
                EpollEvent::empty(),
                EpollEvent::empty(),
                EpollEvent::empty(),
            ],
            nb_events: 0,
            cur_event: 0,
        }
    }
}

impl Iterator for PollEvents {
    type Item = PollEvent;

    fn next(&mut self) -> Option<Self::Item> {
        // No more slot to process, end of iterator.
        if self.cur_event >= self.nb_events {
            return None;
        }

        let slot = &mut self.events[self.cur_event];
        match slot.data() {
            DEVICE_ID => {
                // Figure out which event to return next, if any for this slot.
                if slot.events().contains(EpollFlags::EPOLLOUT) {
                    *slot = EpollEvent::new(
                        slot.events().difference(EpollFlags::EPOLLOUT),
                        slot.data(),
                    );
                    Some(PollEvent::Device(DeviceEvent::OutputReady))
                } else if slot.events().contains(EpollFlags::EPOLLIN) {
                    *slot =
                        EpollEvent::new(slot.events().difference(EpollFlags::EPOLLIN), slot.data());
                    Some(PollEvent::Device(DeviceEvent::CaptureReady))
                } else if slot.events().contains(EpollFlags::EPOLLPRI) {
                    *slot = EpollEvent::new(
                        slot.events().difference(EpollFlags::EPOLLPRI),
                        slot.data(),
                    );
                    Some(PollEvent::Device(DeviceEvent::V4L2Event))
                } else {
                    // If no more events for this slot, try the next one.
                    self.cur_event += 1;
                    self.next()
                }
            }
            waker_id @ FIRST_WAKER_ID..=LAST_WAKER_ID => {
                self.cur_event += 1;
                Some(PollEvent::Waker(waker_id as u32))
            }
            _ => panic!("Unregistered token returned by epoll_wait!"),
        }
    }
}

pub struct Waker {
    fd: File,
}

impl Waker {
    fn new() -> io::Result<Self> {
        let fd = eventfd(0, EfdFlags::EFD_CLOEXEC | EfdFlags::EFD_NONBLOCK)?;

        Ok(Waker { fd: File::from(fd) })
    }

    /// Users will want to use the `wake()` method on an `Arc<Waker>`.
    fn wake_direct(&self) -> io::Result<()> {
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
        match (&self.fd).read(&mut buf).map(|_| ()) {
            Ok(_) => Ok(()),
            // If the counter was already zero, it is already reset so this is
            // not an error.
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => Ok(()),
            Err(e) => Err(e),
        }
    }
}

impl Wake for Waker {
    fn wake(self: Arc<Self>) {
        self.wake_direct().unwrap_or_else(|e| {
            error!("Failed to signal Waker: {}", e);
        });
    }
}

pub struct Poller {
    device: Arc<Device>,
    wakers: BTreeMap<u32, Arc<Waker>>,
    epoll: Epoll,

    // Whether or not to listen to specific device events.
    capture_enabled: bool,
    output_enabled: bool,
    events_enabled: bool,

    // If set, incremented every time we wake up from a poll.
    poll_wakeups_counter: Option<Arc<AtomicUsize>>,
}

/// Wakers IDs range.
const FIRST_WAKER_ID: u64 = 0;
const LAST_WAKER_ID: u64 = DEVICE_ID - 1;
/// Give us a comfortable range of 4 billion ids usable for wakers.
const DEVICE_ID: u64 = 1 << 32;

#[derive(Debug, Error)]
pub enum PollError {
    #[error("error during call to epoll_wait: {0}")]
    EPollWait(nix::Error),
    #[error("error while resetting the waker: {0}")]
    WakerReset(io::Error),
    #[error("V4L2 device returned EPOLLERR")]
    V4L2Device,
}

impl Poller {
    pub fn new(device: Arc<Device>) -> nix::Result<Self> {
        let epoll = Epoll::new(EpollCreateFlags::EPOLL_CLOEXEC)?;

        // Register our device.
        // There is a bug in some Linux kernels (at least 5.9 and older) where EPOLLIN
        // and EPOLLOUT events wont be signaled to epoll if the first call to epoll did
        // not include at least one of EPOLLIN or EPOLLOUT as desired events.
        // Make sure we don't fall into this trap by registering EPOLLIN first and doing
        // a dummy poll call. This call will immediately return with an error because the
        // CAPTURE queue is not streaming, but it will set the right hooks in the kernel
        // and we can now reconfigure our events to only include EPOLLPRI and have poll
        // working as expected.
        epoll.add(&device, EpollEvent::new(EpollFlags::EPOLLIN, DEVICE_ID))?;
        // This call should return an EPOLLERR event immediately. But it will
        // also ensure that the CAPTURE and OUTPUT poll handlers are registered
        // in the kernel for our device.
        epoll.wait(&mut [EpollEvent::empty()], 10)?;
        // Now reset our device events. We must keep it registered for the
        // workaround's effect to persist.
        epoll.modify(
            &device,
            &mut EpollEvent::new(EpollFlags::empty(), DEVICE_ID),
        )?;

        Ok(Poller {
            device,
            wakers: BTreeMap::new(),
            epoll,
            capture_enabled: false,
            output_enabled: false,
            events_enabled: false,
            poll_wakeups_counter: None,
        })
    }

    /// Create a `Waker` with identifier `id` and start polling on it. Returns
    /// the `Waker` if successful, or an error if `id` was already in use or the
    /// waker could not be polled on.
    pub fn add_waker(&mut self, id: u32) -> io::Result<Arc<Waker>> {
        match self.wakers.entry(id) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                let waker = Waker::new()?;

                self.epoll.add(
                    &waker.fd,
                    EpollEvent::new(EpollFlags::EPOLLIN, FIRST_WAKER_ID + id as u64),
                )?;

                let waker = Arc::new(waker);
                entry.insert(Arc::clone(&waker));
                Ok(waker)
            }
            std::collections::btree_map::Entry::Occupied(_) => Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("A waker with id {} is already registered", id),
            )),
        }
    }

    pub fn remove_waker(&mut self, id: u32) -> io::Result<Arc<Waker>> {
        match self.wakers.entry(id) {
            std::collections::btree_map::Entry::Vacant(_) => Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("No waker with id {} in this poller", id),
            )),
            std::collections::btree_map::Entry::Occupied(entry) => {
                self.epoll.delete(&entry.get().fd)?;

                Ok(entry.remove())
            }
        }
    }

    pub fn set_poll_counter(&mut self, poll_wakeup_counter: Arc<AtomicUsize>) {
        self.poll_wakeups_counter = Some(poll_wakeup_counter);
    }

    fn update_device_registration(&mut self) -> nix::Result<()> {
        let mut epoll_flags = EpollFlags::empty();
        if self.capture_enabled {
            epoll_flags.insert(EpollFlags::EPOLLIN);
        }
        if self.output_enabled {
            epoll_flags.insert(EpollFlags::EPOLLOUT);
        }
        if self.events_enabled {
            epoll_flags.insert(EpollFlags::EPOLLPRI);
        }

        let mut epoll_event = EpollEvent::new(epoll_flags, DEVICE_ID);

        self.epoll
            .modify(&self.device, &mut epoll_event)
            .map(|_| ())
    }

    fn set_event(&mut self, event: DeviceEvent, enable: bool) -> nix::Result<()> {
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
    pub fn enable_event(&mut self, event: DeviceEvent) -> nix::Result<()> {
        self.set_event(event, true)
    }

    /// Disable listening to (and reporting of) `event`.
    pub fn disable_event(&mut self, event: DeviceEvent) -> nix::Result<()> {
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

    pub fn poll(&mut self, duration: Option<std::time::Duration>) -> Result<PollEvents, PollError> {
        let mut events = PollEvents::new();
        let duration: isize = match duration {
            None => -1,
            Some(d) => d.as_millis() as isize,
        };

        events.nb_events = self
            .epoll
            .wait(&mut events.events, duration)
            .map_err(PollError::EPollWait)?;

        // Update our wake up stats
        if let Some(wakeup_counter) = &self.poll_wakeups_counter {
            wakeup_counter.fetch_add(1, Ordering::SeqCst);
        }

        // Reset all the wakers that have been signaled.
        for event in &events.events[0..events.nb_events] {
            if event.data() <= LAST_WAKER_ID {
                match self.wakers.get(&(event.data() as u32)) {
                    Some(waker) => waker.reset().map_err(PollError::WakerReset)?,
                    None => warn!("Unregistered waker has been signaled."),
                }
            }
        }

        for event in &events.events[0..events.nb_events] {
            if event.data() == DEVICE_ID && event.events().contains(EpollFlags::EPOLLERR) {
                error!("V4L2 device returned EPOLLERR!");
                return Err(PollError::V4L2Device);
            }
        }

        Ok(events)
    }
}

impl AsFd for Poller {
    fn as_fd(&self) -> BorrowedFd {
        self.epoll.0.as_fd()
    }
}

#[cfg(test)]
mod tests {
    use super::{DeviceEvent::*, PollEvent::*, PollEvents};
    use super::{DEVICE_ID, FIRST_WAKER_ID};
    use nix::sys::epoll::{EpollEvent, EpollFlags};

    #[test]
    fn test_pollevents_iterator() {
        let mut poll_events = PollEvents::new();
        assert_eq!(poll_events.next(), None);

        // Single device events
        let mut poll_events = PollEvents::new();
        poll_events.events[0] = EpollEvent::new(EpollFlags::EPOLLIN, DEVICE_ID);
        poll_events.nb_events = 1;
        assert_eq!(poll_events.next(), Some(Device(CaptureReady)));
        assert_eq!(poll_events.next(), None);

        let mut poll_events = PollEvents::new();
        poll_events.events[0] = EpollEvent::new(EpollFlags::EPOLLOUT, DEVICE_ID);
        poll_events.nb_events = 1;
        assert_eq!(poll_events.next(), Some(Device(OutputReady)));
        assert_eq!(poll_events.next(), None);

        let mut poll_events = PollEvents::new();
        poll_events.events[0] = EpollEvent::new(EpollFlags::EPOLLPRI, DEVICE_ID);
        poll_events.nb_events = 1;
        assert_eq!(poll_events.next(), Some(Device(V4L2Event)));
        assert_eq!(poll_events.next(), None);

        // Multiple device events in one event
        let mut poll_events = PollEvents::new();
        poll_events.events[0] =
            EpollEvent::new(EpollFlags::EPOLLPRI | EpollFlags::EPOLLOUT, DEVICE_ID);
        poll_events.nb_events = 1;
        assert_eq!(poll_events.next(), Some(Device(OutputReady)));
        assert_eq!(poll_events.next(), Some(Device(V4L2Event)));
        assert_eq!(poll_events.next(), None);

        // Separated device events
        let mut poll_events = PollEvents::new();
        poll_events.events[0] = EpollEvent::new(EpollFlags::EPOLLIN, DEVICE_ID);
        poll_events.events[1] =
            EpollEvent::new(EpollFlags::EPOLLPRI | EpollFlags::EPOLLOUT, DEVICE_ID);
        poll_events.nb_events = 2;
        assert_eq!(poll_events.next(), Some(Device(CaptureReady)));
        assert_eq!(poll_events.next(), Some(Device(OutputReady)));
        assert_eq!(poll_events.next(), Some(Device(V4L2Event)));
        assert_eq!(poll_events.next(), None);

        // Single waker event
        let mut poll_events = PollEvents::new();
        poll_events.events[0] = EpollEvent::new(EpollFlags::empty(), FIRST_WAKER_ID);
        poll_events.nb_events = 1;
        assert_eq!(poll_events.next(), Some(Waker(0)));
        assert_eq!(poll_events.next(), None);

        // Multiple waker events
        let mut poll_events = PollEvents::new();
        poll_events.events[0] = EpollEvent::new(EpollFlags::empty(), FIRST_WAKER_ID + 20);
        poll_events.events[1] = EpollEvent::new(EpollFlags::empty(), FIRST_WAKER_ID + 42);
        poll_events.events[2] = EpollEvent::new(EpollFlags::empty(), FIRST_WAKER_ID);
        poll_events.nb_events = 3;
        assert_eq!(poll_events.next(), Some(Waker(20)));
        assert_eq!(poll_events.next(), Some(Waker(42)));
        assert_eq!(poll_events.next(), Some(Waker(0)));
        assert_eq!(poll_events.next(), None);

        // Wakers and device events
        let mut poll_events = PollEvents::new();
        poll_events.events[0] = EpollEvent::new(EpollFlags::empty(), FIRST_WAKER_ID + 20);
        poll_events.events[1] = EpollEvent::new(EpollFlags::empty(), FIRST_WAKER_ID + 42);
        poll_events.events[2] =
            EpollEvent::new(EpollFlags::EPOLLPRI | EpollFlags::EPOLLIN, DEVICE_ID);
        poll_events.events[3] = EpollEvent::new(EpollFlags::empty(), FIRST_WAKER_ID);
        poll_events.nb_events = 4;
        assert_eq!(poll_events.next(), Some(Waker(20)));
        assert_eq!(poll_events.next(), Some(Waker(42)));
        assert_eq!(poll_events.next(), Some(Device(CaptureReady)));
        assert_eq!(poll_events.next(), Some(Device(V4L2Event)));
        assert_eq!(poll_events.next(), Some(Waker(0)));
        assert_eq!(poll_events.next(), None);
    }
}

use crate::{
    decoder::{
        stateful::{CaptureThreadResponse, DecoderCommand, DecoderEvent, DrainError},
        DecoderEventCallback, FormatChangedCallback, FormatChangedReply,
    },
    device::{
        poller::{DeviceEvent, PollEvent, Poller, Waker},
        queue::{
            self,
            direction::Capture,
            handles_provider::HandlesProvider,
            qbuf::{
                get_free::GetFreeCaptureBuffer, get_indexed::GetCaptureBufferByIndex,
                CaptureQueueable,
            },
            BuffersAllocated, Queue, QueueInit,
        },
        AllocatedQueue, Device, Stream, TryDequeue,
    },
    ioctl::{self, SelectionTarget},
};

use std::{
    io,
    sync::{mpsc, Arc},
    task::Wake,
};

use log::{debug, error, trace, warn};
use thiserror::Error;

/// Check if `device` has a dynamic resolution change event pending.
///
/// Dequeues all pending V4L2 events and returns `true` if a
/// SRC_CHANGE_EVENT (indicating a format change on the CAPTURE queue) was
/// detected. This consumes all the event, meaning that if this method
/// returned `true` once it will return `false` until a new resolution
/// change happens in the stream.
fn is_drc_event_pending(device: &Device) -> Result<bool, ioctl::DqEventError> {
    let mut drc_pending = false;

    loop {
        // TODO what if we used an iterator here?
        let event = match ioctl::dqevent(device) {
            Ok(event) => event,
            Err(ioctl::DqEventError::NotReady) => return Ok(drc_pending),
            Err(e) => return Err(e),
        };

        match event {
            ioctl::Event::SrcChangeEvent(changes) => {
                if changes.contains(ioctl::SrcChanges::RESOLUTION) {
                    debug!("Received resolution change event");
                    drc_pending = true;
                }
            }
            ioctl::Event::Eos => {
                debug!("Received EOS event");
            }
        }
    }
}

enum CaptureQueue<P: HandlesProvider> {
    AwaitingResolution {
        capture_queue: Queue<Capture, QueueInit>,
    },
    Decoding {
        capture_queue: Queue<Capture, BuffersAllocated<P::HandleType>>,
        provider: P,
        cap_buffer_waker: Arc<Waker>,
        // TODO not super elegant...
        blocking_drain_in_progress: bool,
    },
}

pub(super) struct CaptureThread<P, DecoderEventCb, FormatChangedCb>
where
    P: HandlesProvider,
    DecoderEventCb: DecoderEventCallback<P>,
    FormatChangedCb: FormatChangedCallback<P>,
{
    device: Arc<Device>,
    capture_queue: CaptureQueue<P>,
    pub(super) poller: Poller,

    event_cb: DecoderEventCb,
    set_capture_format_cb: FormatChangedCb,

    // Waker signaled when the main thread has commands pending for us.
    pub(super) command_waker: Arc<Waker>,
    // Receiver we read commands from when `command_waker` is signaled.
    command_receiver: mpsc::Receiver<DecoderCommand>,
    // Sender we use to send status messages after receiving commands from the
    // main thread.
    response_sender: mpsc::Sender<CaptureThreadResponse>,
}

#[derive(Debug, Error)]
enum UpdateCaptureError {
    #[error("error while enabling poller events: {0}")]
    PollerEvents(io::Error),
    #[error("error while removing CAPTURE waker: {0}")]
    RemoveWaker(io::Error),
    #[error("error while stopping CAPTURE queue: {0}")]
    Streamoff(#[from] ioctl::StreamOffError),
    #[error("error while freeing CAPTURE buffers: {0}")]
    FreeBuffers(#[from] ioctl::ReqbufsError),
    #[error("error while obtaining CAPTURE format: {0}")]
    GFmt(#[from] ioctl::GFmtError),
    #[error("error while obtaining selection target from CAPTURE queue: {0}")]
    GSelection(#[from] ioctl::GSelectionError),
    #[error("error while running the CAPTURE format callback: {0}")]
    Callback(#[from] anyhow::Error),
    #[error("error while requesting CAPTURE buffers: {0}")]
    RequestBuffers(#[from] queue::RequestBuffersError),
    #[error("error while adding the CAPTURE buffer waker: {0}")]
    AddWaker(io::Error),
    #[error("error while streaming CAPTURE queue: {0}")]
    StreamOn(#[from] ioctl::StreamOnError),
}

const CAPTURE_READY: u32 = 1;
const COMMAND_WAITING: u32 = 2;

#[derive(Debug, Error)]
enum ProcessEventsError {
    #[error("error while dequeueing event")]
    DqEvent(#[from] ioctl::DqEventError),
    #[error("error while requesting buffers")]
    RequestBuffers(#[from] queue::RequestBuffersError),
    #[error("error while updating CAPTURE format")]
    UpdateCapture(#[from] UpdateCaptureError),
}

impl<P, DecoderEventCb, FormatChangedCb> CaptureThread<P, DecoderEventCb, FormatChangedCb>
where
    P: HandlesProvider,
    DecoderEventCb: DecoderEventCallback<P>,
    FormatChangedCb: FormatChangedCallback<P>,
    for<'a> Queue<Capture, BuffersAllocated<P::HandleType>>:
        GetFreeCaptureBuffer<'a, P::HandleType> + GetCaptureBufferByIndex<'a, P::HandleType>,
{
    pub(super) fn new(
        device: &Arc<Device>,
        capture_queue: Queue<Capture, QueueInit>,
        event_cb: DecoderEventCb,
        set_capture_format_cb: FormatChangedCb,
        command_receiver: mpsc::Receiver<DecoderCommand>,
        response_sender: mpsc::Sender<CaptureThreadResponse>,
    ) -> io::Result<Self> {
        // Start by only listening to V4L2 events in order to catch the initial
        // resolution change, and to the stop waker in case the user had a
        // change of heart about decoding something now.
        let mut poller = Poller::new(Arc::clone(device))?;
        poller.enable_event(DeviceEvent::V4L2Event)?;
        let command_waker = poller.add_waker(COMMAND_WAITING)?;

        let decoder_thread = CaptureThread {
            device: Arc::clone(device),
            capture_queue: CaptureQueue::AwaitingResolution { capture_queue },
            poller,
            event_cb,
            set_capture_format_cb,
            command_waker,
            command_receiver,
            response_sender,
        };

        Ok(decoder_thread)
    }

    fn send_response(&self, response: CaptureThreadResponse) {
        trace!("Sending response: {:?}", response);

        self.response_sender.send(response).unwrap();
    }

    fn drain(&mut self, blocking: bool) {
        trace!("Processing Drain({}) command", blocking);
        let response = match &mut self.capture_queue {
            // We cannot initiate the flush sequence before receiving the initial
            // resolution.
            CaptureQueue::AwaitingResolution { .. } => {
                Some(CaptureThreadResponse::DrainDone(Err(DrainError::TryAgain)))
            }
            CaptureQueue::Decoding {
                blocking_drain_in_progress,
                ..
            } => {
                // We can receive the LAST buffer, send the STOP command
                // and exit the loop once the buffer with the LAST tag is received.
                ioctl::decoder_cmd::<_, ()>(&*self.device, ioctl::DecoderCmd::stop()).unwrap();
                if blocking {
                    // If we are blocking, we will send the answer when the drain
                    // is completed.
                    *blocking_drain_in_progress = true;
                    None
                } else {
                    // If not blocking, send the response now so the client can keep going.
                    Some(CaptureThreadResponse::DrainDone(Ok(false)))
                }
            }
        };

        if let Some(response) = response {
            self.send_response(response);
        }
    }

    fn flush(&mut self) {
        trace!("Processing flush command");
        match &mut self.capture_queue {
            CaptureQueue::AwaitingResolution { .. } => {}
            CaptureQueue::Decoding {
                capture_queue,
                blocking_drain_in_progress,
                ..
            } => {
                // Stream the capture queue off and back on, dropping any queued
                // buffer, and making the decoder ready to work again if it was
                // halted.
                capture_queue.stream_off().unwrap();
                capture_queue.stream_on().unwrap();
                *blocking_drain_in_progress = false;
            }
        }

        self.send_response(CaptureThreadResponse::FlushDone(Ok(())));
        self.enqueue_capture_buffers()
    }

    fn enqueue_capture_buffers(&mut self) {
        trace!("Queueing available CAPTURE buffers");
        let (capture_queue, provider, cap_buffer_waker) = match &mut self.capture_queue {
            // Capture queue is not set up yet, no buffers to queue.
            CaptureQueue::AwaitingResolution { .. } => return,
            CaptureQueue::Decoding {
                capture_queue,
                provider,
                cap_buffer_waker,
                ..
            } => (capture_queue, provider, cap_buffer_waker),
        };

        // Requeue all available CAPTURE buffers.
        'enqueue: while let Some(handles) = provider.get_handles(cap_buffer_waker) {
            // TODO potential problem: the handles will be dropped if no V4L2 buffer
            // is available. There is no guarantee that the provider will get them back
            // in this case (e.g. with the C FFI).
            let buffer = match provider.get_suitable_buffer_for(&handles, capture_queue) {
                Ok(buffer) => buffer,
                // It is possible that we run out of V4L2 buffers if there are more handles than
                // buffers allocated. One example of this scenario is the `MmapProvider` which has
                // an infinite number of handles. Break out of the loop when this happens - we will
                // be called again the next time a CAPTURE buffer becomes available.
                Err(queue::handles_provider::GetSuitableBufferError::TryGetFree(
                    queue::qbuf::get_free::GetFreeBufferError::NoFreeBuffer,
                )) => {
                    break 'enqueue;
                }
                Err(e) => {
                    error!("Could not find suitable buffer for handles: {}", e);
                    warn!("Handles potentially lost due to no V4L2 buffer being available");
                    break 'enqueue;
                }
            };
            match buffer.queue_with_handles(handles) {
                Ok(()) => (),
                Err(e) => error!("Error while queueing CAPTURE buffer: {}", e),
            }
        }
    }

    fn process_v4l2_event(mut self) -> Self {
        trace!("Processing V4L2 event");
        match self.capture_queue {
            CaptureQueue::AwaitingResolution { .. } => {
                if is_drc_event_pending(&self.device).unwrap() {
                    self = self.update_capture_format().unwrap()
                }
            }
            CaptureQueue::Decoding { .. } => unreachable!(),
        }

        self
    }

    fn update_capture_format(mut self) -> Result<Self, UpdateCaptureError> {
        debug!("Updating CAPTURE format");
        // First reset the capture queue to the `Init` state if needed.
        let mut capture_queue = match self.capture_queue {
            // Initial resolution
            CaptureQueue::AwaitingResolution { capture_queue } => {
                // Stop listening to V4L2 events. We will check them when we get
                // a buffer with the LAST flag.
                self.poller
                    .disable_event(DeviceEvent::V4L2Event)
                    .map_err(Into::<io::Error>::into)
                    .map_err(UpdateCaptureError::PollerEvents)?;
                // Listen to CAPTURE buffers being ready to dequeue, as we will
                // be streaming soon.
                self.poller
                    .enable_event(DeviceEvent::CaptureReady)
                    .map_err(Into::<io::Error>::into)
                    .map_err(UpdateCaptureError::PollerEvents)?;
                capture_queue
            }
            // Dynamic resolution change
            CaptureQueue::Decoding { capture_queue, .. } => {
                // Remove the waker for the previous buffers pool, as we will
                // get a new set of buffers.
                self.poller
                    .remove_waker(CAPTURE_READY)
                    .map_err(UpdateCaptureError::RemoveWaker)?;
                // Deallocate the queue and return it to the `Init` state. Good
                // as new!
                capture_queue.stream_off()?;
                capture_queue.free_buffers()?.queue
            }
        };

        // Now get the parameters of the new format and build our new CAPTURE
        // queue.

        // TODO use the proper control to get the right value.
        let min_num_buffers = 4usize;
        debug!("Stream requires {} capture buffers", min_num_buffers);

        let visible_rect = capture_queue.get_selection(SelectionTarget::Compose)?;
        debug!(
            "Visible rectangle: ({}, {}), {}x{}",
            visible_rect.left, visible_rect.top, visible_rect.width, visible_rect.height
        );

        // Let the client adjust the new format and give us the handles provider.
        let FormatChangedReply {
            provider,
            mem_type,
            num_buffers,
        } = (self.set_capture_format_cb)(
            capture_queue.change_format()?,
            visible_rect,
            min_num_buffers,
        )?;

        debug!("Client requires {} capture buffers", num_buffers);

        // Allocate the new CAPTURE buffers and get ourselves a new waker for
        // returning buffers.
        let capture_queue =
            capture_queue.request_buffers_generic::<P::HandleType>(mem_type, num_buffers as u32)?;
        let cap_buffer_waker = self
            .poller
            .add_waker(CAPTURE_READY)
            .map_err(UpdateCaptureError::AddWaker)?;

        // Ready to decode - signal the waker so we immediately enqueue buffers
        // and start streaming.
        cap_buffer_waker.wake_by_ref();
        capture_queue.stream_on()?;

        Ok(Self {
            capture_queue: CaptureQueue::Decoding {
                capture_queue,
                provider,
                cap_buffer_waker,
                blocking_drain_in_progress: false,
            },
            ..self
        })
    }

    /// Attempt to dequeue and process a single CAPTURE buffer.
    ///
    /// If a buffer can be dequeued, then the following processing takes place:
    /// * Invoke the event callback with a `FrameDecoded` event containing the
    ///   dequeued buffer,
    /// * If the buffer has the LAST flag set:
    ///   * If a resolution change event is pending, start the resolution change
    ///     procedure,
    ///   * If a resolution change event is not pending, invoke the event
    ///     callback with an 'EndOfStream` event,
    ///   * If a blocking drain was in progress, complete it.
    fn dequeue_capture_buffer(mut self) -> Self {
        trace!("Dequeueing decoded CAPTURE buffers");
        let (capture_queue, cap_buffer_waker, blocking_drain_in_progress) =
            match &mut self.capture_queue {
                CaptureQueue::AwaitingResolution { .. } => unreachable!(),
                CaptureQueue::Decoding {
                    capture_queue,
                    cap_buffer_waker,
                    blocking_drain_in_progress,
                    ..
                } => (capture_queue, cap_buffer_waker, blocking_drain_in_progress),
            };

        let mut cap_buf = match capture_queue.try_dequeue() {
            Ok(cap_buf) => cap_buf,
            Err(e) => {
                warn!(
                    "Expected a CAPTURE buffer but none available, possible driver bug: {}",
                    e
                );
                return self;
            }
        };

        let is_last = cap_buf.data.is_last();

        // Add a drop callback to the dequeued buffer so we
        // re-queue it as soon as it is dropped.
        let cap_waker = Arc::clone(cap_buffer_waker);
        cap_buf.add_drop_callback(move |_dqbuf| {
            // Intentionally ignore the result here.
            cap_waker.wake();
        });

        // Pass buffers to the client
        (self.event_cb)(DecoderEvent::FrameDecoded(cap_buf));

        if is_last {
            debug!("CAPTURE buffer marked with LAST flag");
            if is_drc_event_pending(&self.device).unwrap() {
                debug!("DRC event pending, updating CAPTURE format");
                self = self.update_capture_format().unwrap()
            }
            // No DRC event pending, this is the end of the stream.
            // We need to stop and restart the CAPTURE queue, otherwise
            // it will keep signaling buffers as ready and dequeueing
            // them will return `EPIPE`.
            else {
                debug!("No DRC event pending, restarting capture queue");
                // We are supposed to be able to run the START command
                // instead, but with vicodec the CAPTURE queue reports
                // as ready in subsequent polls() and DQBUF returns
                // -EPIPE...
                capture_queue.stream_off().unwrap();
                capture_queue.stream_on().unwrap();
                (self.event_cb)(DecoderEvent::EndOfStream);
                if *blocking_drain_in_progress {
                    debug!("Signaling end of blocking drain");
                    *blocking_drain_in_progress = false;
                    self.send_response(CaptureThreadResponse::DrainDone(Ok(true)));
                }
            }
        }

        self
    }

    pub(super) fn run(mut self) -> Self {
        'mainloop: loop {
            if let CaptureQueue::Decoding { capture_queue, .. } = &self.capture_queue {
                match capture_queue.num_queued_buffers() {
                    // If there are no buffers on the CAPTURE queue, poll() will return
                    // immediately with EPOLLERR and we would loop indefinitely.
                    // Prevent this by temporarily disabling polling the CAPTURE queue
                    // in such cases.
                    0 => {
                        self.poller
                            .disable_event(DeviceEvent::CaptureReady)
                            .unwrap();
                    }
                    // If device polling was disabled and we have buffers queued, we
                    // can reenable it as poll will now wait for a CAPTURE buffer to
                    // be ready for dequeue.
                    _ => {
                        self.poller.enable_event(DeviceEvent::CaptureReady).unwrap();
                    }
                }
            }

            trace!("Polling...");
            let events = match self.poller.poll(None) {
                Ok(events) => events,
                Err(e) => {
                    error!("Polling failure, exiting capture thread: {}", e);
                    break 'mainloop;
                }
            };
            for event in events {
                self = match event {
                    PollEvent::Device(DeviceEvent::V4L2Event) => self.process_v4l2_event(),
                    PollEvent::Device(DeviceEvent::CaptureReady) => self.dequeue_capture_buffer(),
                    PollEvent::Waker(CAPTURE_READY) => {
                        self.enqueue_capture_buffers();
                        self
                    }
                    PollEvent::Waker(COMMAND_WAITING) => {
                        loop {
                            let command =
                                match self.command_receiver.recv_timeout(Default::default()) {
                                    Ok(command) => command,
                                    Err(mpsc::RecvTimeoutError::Timeout) => break,
                                    Err(e) => {
                                        error!("Error while reading decoder command: {}", e);
                                        break;
                                    }
                                };
                            match command {
                                DecoderCommand::Drain(blocking) => self.drain(blocking),
                                DecoderCommand::Flush => self.flush(),
                                DecoderCommand::Stop => {
                                    trace!("Processing stop command");
                                    break 'mainloop;
                                }
                            }
                        }
                        self
                    }
                    _ => panic!("Unexpected event!"),
                }
            }
        }

        // Return the decoder to the awaiting resolution state.
        match self.capture_queue {
            CaptureQueue::AwaitingResolution { .. } => self,
            CaptureQueue::Decoding { capture_queue, .. } => Self {
                capture_queue: CaptureQueue::AwaitingResolution {
                    capture_queue: {
                        capture_queue.stream_off().unwrap();
                        capture_queue.free_buffers().unwrap().queue
                    },
                },
                poller: {
                    let mut poller = self.poller;
                    poller.disable_event(DeviceEvent::CaptureReady).unwrap();
                    poller.enable_event(DeviceEvent::V4L2Event).unwrap();
                    poller.remove_waker(CAPTURE_READY).unwrap();
                    poller
                },
                ..self
            },
        }
    }
}

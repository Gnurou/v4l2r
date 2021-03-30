#![allow(non_camel_case_types)]

use log::{error, trace};
use std::{
    collections::VecDeque,
    os::{
        raw::c_int,
        unix::io::{AsRawFd, RawFd},
    },
    sync::{Arc, Mutex},
};
use v4l2r::{
    bindings,
    device::{
        poller::Waker,
        queue::{
            handles_provider::{GetSuitableBufferError, HandlesProvider},
            qbuf::{
                get_free::GetFreeCaptureBuffer, get_indexed::GetCaptureBufferByIndex,
                CaptureQueueableProvider,
            },
        },
    },
    memory::{BufferHandles, DMABufHandle, DMABufSource, MemoryType, PrimitiveBufferHandles},
};

/// The simplest type used to represent a DMABUF fd. It does not take ownership
/// of the FD at any time and does not close it ; thus the using code is
/// responsible for managing the given FD's lifetime.
///
/// Since no ownership is taken at all, this is mostly useful for using DMABUFs
/// managed from unsafe code, i.e. code that calls into us using the C ffi.
#[derive(Debug)]
pub struct DMABufFd {
    fd: RawFd,
    len: u64,
}

impl DMABufFd {
    /// Create a new `RawFd` to be used with the DMABUF API.
    /// `fd` is the unix raw fd, `len` is the size of the memory behind it (not
    /// just the amound of used data, but the whole size of the buffer).
    /// No ownership is taken over `fd`, which will not be closed as the `RawFd`
    /// is dropped ; thus the caller is responsible for managing its lifetime.
    pub fn new(fd: RawFd, len: u64) -> DMABufFd {
        DMABufFd { fd, len }
    }
}

impl AsRawFd for DMABufFd {
    fn as_raw_fd(&self) -> RawFd {
        self.fd
    }
}

impl DMABufSource for DMABufFd {
    fn len(&self) -> u64 {
        self.len
    }
}

/// A struct representing a set of buffers to which decoded frames will be
/// output.
#[derive(Debug, Default)]
#[repr(C)]
pub struct v4l2r_video_frame {
    /// Identifier of the frame. Each frame of the provider must have a unique
    /// identifier the between 0 and 31 included, and that identifier must
    /// persist across reuse of the same frame.
    pub id: u32,
    /// Number of entries in `fds`, e.g. the number of planes in this frame.
    pub num_planes: usize,
    /// DMABUF FDs of the planes for this frame.
    pub planes: [c_int; 4],
}

#[derive(Debug, Clone, Copy, Default)]
pub struct VideoFrameMemoryType;

impl From<VideoFrameMemoryType> for MemoryType {
    fn from(_: VideoFrameMemoryType) -> Self {
        MemoryType::DMABuf
    }
}

impl BufferHandles for v4l2r_video_frame {
    type SupportedMemoryType = VideoFrameMemoryType;

    fn len(&self) -> usize {
        self.num_planes
    }

    fn fill_v4l2_plane(&self, index: usize, plane: &mut bindings::v4l2_plane) {
        plane.m.fd = self.planes[index];
        // We don't need to set plane.m.length as these buffers are meant for
        // the CAPTURE queue.
    }
}

impl PrimitiveBufferHandles for v4l2r_video_frame {
    // TODO: Uh? This is bullocks but somehow it compiles??
    type HandleType = DMABufHandle<DMABufFd>;

    const MEMORY_TYPE: Self::SupportedMemoryType = VideoFrameMemoryType;
}

struct VideoFrameProviderInternal {
    frames: VecDeque<v4l2r_video_frame>,
    waker: Option<Arc<Waker>>,
}

/// A way for the client-side to provide frames to be decoded into in the form
/// of video frames. A new provider will be passed by the output format change
/// callback every time the output format is changing. The client must pass
/// valid frames of the format specified by the format change callback using
/// [`v4l2r_video_frame_provider_queue_frame`]. These frames will be decoded
/// into and then passed as parameters of the frame output callback.
pub struct v4l2r_video_frame_provider {
    d: Mutex<VideoFrameProviderInternal>,
}

impl v4l2r_video_frame_provider {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        v4l2r_video_frame_provider {
            d: Mutex::new(VideoFrameProviderInternal {
                frames: VecDeque::new(),
                waker: None,
            }),
        }
    }
}

impl HandlesProvider for v4l2r_video_frame_provider {
    type HandleType = v4l2r_video_frame;

    // TODO BUG: if the V4L2 buffer queue fails for some reason, there
    // is no guarantee that the handles will return to the provider, and
    // they might be definitively lost!
    // In this case this is probably not too serious as the C side, which
    // manages the DMABUFs, should receive and error and cancel decoding.
    // Ideally the handles would be a C++ object that we just pass around,
    // and which destructor would be called even if the Rust side drops it.
    fn get_handles(&self, waker: &Arc<Waker>) -> Option<Self::HandleType> {
        let mut d = self.d.lock().unwrap();
        match d.frames.pop_front() {
            Some(handles) => Some(handles),
            None => {
                d.waker = Some(Arc::clone(waker));
                None
            }
        }
    }

    fn get_suitable_buffer_for<'a, Q>(
        &self,
        handles: &Self::HandleType,
        queue: &'a Q,
    ) -> Result<
        <Q as CaptureQueueableProvider<'a, Self::HandleType>>::Queueable,
        GetSuitableBufferError,
    >
    where
        Q: GetCaptureBufferByIndex<'a, Self::HandleType>
            + GetFreeCaptureBuffer<'a, Self::HandleType>,
    {
        trace!("Getting suitable buffer for frame {}", handles.id);

        // Try to get the buffer with the same id as our frame. If that is not
        // possible, fall back to returning any free buffer.
        let buffer = queue.try_get_buffer(handles.id as usize).or_else(|e| {
            error!(
                "failed to obtain CAPTURE buffer {} by index: {}",
                handles.id, e
            );
            error!("falling back to getting the first available buffer.");
            queue.try_get_free_buffer()
        })?;

        Ok(buffer)
    }
}

/// Make `frame` available to `provider` for being decoded into.
///
/// `frame` must be a valid frame of the format provided by the output format
/// change callback from which `provider` originated. `frame` will reappear as
/// an argument of the frame output callback once it has been decoded into, and
/// will remain untouched by the decoder until the client passes it to this
/// function again.
///
/// Returns `true` upon success, `false` if the provided frame had an invalid
/// index or the decoder thread could not be awakened.
///
/// This function can safely be called from any thread.
///
/// # Safety
///
/// `provider` must be a valid pointer provided by the resolution change
/// callback. It must *not* be used after the resolution change callback is
/// called again.
#[no_mangle]
pub unsafe extern "C" fn v4l2r_video_frame_provider_queue_frame(
    provider: *const v4l2r_video_frame_provider,
    frame: v4l2r_video_frame,
) -> bool {
    trace!("Queueing output frame: {:?}", frame);
    assert!(!provider.is_null());
    let provider = &*provider;

    if frame.id >= bindings::VIDEO_MAX_FRAME {
        error!("Invalid frame id {}, aborting queue.", frame.id);
        return false;
    }

    let mut provider = provider.d.lock().unwrap();
    provider.frames.push_back(frame);
    if let Some(waker) = provider.waker.take() {
        match waker.wake() {
            Err(e) => {
                error!("Error signaling output frame queued waker: {}", e);
                false
            }
            Ok(()) => true,
        }
    } else {
        true
    }
}

/// Delete a video frame provider.
///
/// # Safety
///
/// `provider` must be a provider previously passed through the
/// `v4l2r_decoder_format_changed_event`. There are only two times when calling
/// this function is valid:
///
/// 1) After another `v4l2r_decoder_format_changed_event` has been received, the
///    old provider can be disposed of after the client is sure it won't make
///    any access.
/// 2) After the video decoder has been stopped and destroyed, the client must
///    drop the last provider it received (if any) itself.
#[no_mangle]
pub unsafe extern "C" fn v4l2r_video_frame_provider_drop(
    provider: *const v4l2r_video_frame_provider,
) {
    trace!("Destroying video frame provider: {:p}", provider);
    assert!(!provider.is_null());

    Arc::from_raw(provider);
}

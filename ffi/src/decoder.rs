//! Module for creating and controlling V4L2 decoders.
//!
//! Decoders are created using [`v4l2r_decoder_new`] and remain
//! active until being given to [`v4l2r_decoder_destroy`]. They expect
//! to be fed encoded buffers in the format specified at creation time using
//! [`v4l2r_decoder_decode`].
//!
//! Decoders communicate with the client using an event callback that is invoked
//! on a dedicated thread. This callback signals events of interest, like a
//! frame being decoded, or a change in the output format (due to e.g. a dynamic
//! resolution change). The output format is initially undefined and a format
//! change event will be produced before any frame can be decoded.
#![allow(non_camel_case_types)]

use log::{debug, error, info, warn};
use nix::sys::time::{TimeVal, TimeValLike};
use std::{
    ffi::CStr,
    mem::MaybeUninit,
    os::raw::{c_char, c_int, c_uint, c_void},
    path::Path,
    sync::Arc,
};
use v4l2r::{
    bindings,
    decoder::{
        stateful::{Decoder, Decoding, DrainError},
        CompletedInputBuffer, DecoderEvent, DecoderEventCallback, FormatChangedCallback,
        FormatChangedReply, InputDoneCallback,
    },
    device::queue::{direction::Capture, dqbuf::DqBuffer, qbuf::OutputQueueable, FormatBuilder},
    memory::DmaBufHandle,
    PixelFormat, PlaneLayout, Rect,
};

use crate::memory::{
    v4l2r_video_frame, v4l2r_video_frame_provider, v4l2r_video_frame_provider_queue_frame,
    DmaBufFd, VideoFrameMemoryType,
};

type DynCbDecoder = Decoder<
    Decoding<
        Vec<DmaBufHandle<DmaBufFd>>,
        Arc<v4l2r_video_frame_provider>,
        Box<dyn InputDoneCallback<Vec<DmaBufHandle<DmaBufFd>>>>,
        Box<dyn DecoderEventCallback<Arc<v4l2r_video_frame_provider>>>,
        Box<dyn FormatChangedCallback<Arc<v4l2r_video_frame_provider>>>,
    >,
>;

/// A V4L2 decoder instance.
pub struct v4l2r_decoder {
    decoder: DynCbDecoder,
    // Reference to the video frame provider for our callbacks.
    provider: Option<Arc<v4l2r_video_frame_provider>>,
    // Keep the size of input buffers at hand.
    input_buf_size: u64,
}

/// Callback called when the decoder is done with a buffer submitted using
/// [`v4l2r_decoder_decode`].
///
/// The first argument is the `cb_data` pointer given
/// to [`v4l2r_decoder_new`]. The second argument is the dequeued V4L2 buffer.
/// The client can use the `timestamp.tv_sec` member of `buffer` to match this
/// buffer with the `bitstream_id` parameter of [`v4l2r_decoder_decode`] and
/// understand which buffer has just completed.
///
/// This callback is only called during calls to [`v4l2r_decoder_decode`] and
/// [`v4l2r_decoder_kick`].
pub type v4l2r_decoder_input_done_cb = extern "C" fn(*mut c_void, *const bindings::v4l2_buffer);

#[repr(C)]
pub struct v4l2r_decoder_frame_decoded_event {
    /// Dequeued V4L2 buffer that has produced the frame. Useful to check for
    /// flags and errors.
    buffer: *const bindings::v4l2_buffer,
    /// One of the frames previously made available to the decoder using
    /// [`v4l2r_video_frame_provider_queue_frame`].
    ///
    /// [`v4l2r_video_frame_provider_queue_frame`]:
    /// crate::memory::v4l2r_video_frame_provider_queue_frame
    frame: v4l2r_video_frame,
}

/// Event produced every time the output format of the stream changes.
/// This includes when the initial format is determined by the decoder, and any
/// subsequent dynamic resolution change in the stream.
#[repr(C)]
pub struct v4l2r_decoder_format_changed_event {
    /// New format for decoded frames produced after this event.
    new_format: *mut bindings::v4l2_format,
    /// Visible rectangle for decoded frames produced after this event.
    visible_rect: bindings::v4l2_rect,
    /// Pointer to the video frame provider the client must use to provide
    /// frames to decode into.
    ///
    /// When the client receives this event, it must stop using the previous
    /// video frame provider (if any) as soon as possible and destroy it using
    /// `v4l2r_video_frame_provider_drop`. Any video frame still queued to an
    /// old provider and that has not been seen in a previous `FrameDecoded`
    /// event can be considered as returned to the client. Upon receiving this
    /// event, the client is guaranteed to not receive any frame in the previous
    /// format, or from the previous provider.
    ///
    /// The client is responsible for allocating video frames in the new format
    /// and start giving them to the new provider using
    /// [`v4l2r_video_frame_provider_queue_frame`].
    ///
    /// [`v4l2r_video_frame_provider_queue_frame`]:
    /// crate::memory::v4l2r_video_frame_provider_queue_frame
    new_provider: *const v4l2r_video_frame_provider,
    /// Minimum number of output buffers required by the decoder to operate
    /// properly.
    ///
    /// The client must allocate at least `min_num_frames` (but no more than
    /// 32), otherwise the decoder might starve.
    min_num_frames: c_uint,
}

/// Decoding-related events. These events can be produced at any time between
/// calls to [`v4l2r_decoder_new`] and [`v4l2r_decoder_destroy`] and
/// are passed to the events callback.
#[repr(C)]
pub enum v4l2r_decoder_event {
    // TODO for frames that have a zero-size, just recycle the handles
    // on-the-spot and pass the relevant event instead!
    FrameDecoded(v4l2r_decoder_frame_decoded_event),
    FormatChanged(v4l2r_decoder_format_changed_event),
    EndOfStream,
}

/// Events callback. This callback is guaranteed to always be called from the
/// same thread, i.e. events are completely sequential.
pub type v4l2r_decoder_event_cb = extern "C" fn(*mut c_void, *mut v4l2r_decoder_event);

fn set_capture_format_cb(
    f: FormatBuilder,
    desired_pixel_format: Option<PixelFormat>,
    visible_rect: Rect,
    min_num_buffers: usize,
    decoder: *mut v4l2r_decoder,
    event_cb: v4l2r_decoder_event_cb,
    cb_data: *mut c_void,
) -> anyhow::Result<FormatChangedReply<Arc<v4l2r_video_frame_provider>>> {
    // Safe unless the C part did something funny with the decoder returned by
    // `v4l2r_decoder_new`.
    let decoder = unsafe { decoder.as_mut().unwrap() };
    let mut v4l2_format: bindings::v4l2_format = match desired_pixel_format {
        Some(format) => f.set_pixelformat(format).apply()?,
        None => f.apply()?,
    };

    // Create new memory provider on the heap and update our internal pointer.
    let new_provider = Arc::new(v4l2r_video_frame_provider::new());
    // Reference for our own callbacks.
    decoder.provider = Some(Arc::clone(&new_provider));

    // Reference owned by the client. Will be dropped when it calls
    // `v4l2r_video_frame_provider_drop`.
    let provider_client_ref = Arc::clone(&new_provider);

    // TODO check return value.
    event_cb(
        cb_data,
        &mut v4l2r_decoder_event::FormatChanged(v4l2r_decoder_format_changed_event {
            new_format: &mut v4l2_format,
            visible_rect: visible_rect.into(),
            new_provider: Arc::into_raw(provider_client_ref),
            min_num_frames: min_num_buffers as c_uint,
        }),
    );

    Ok(FormatChangedReply {
        provider: new_provider,
        // TODO: can't the provider report the memory type that it is
        // actually serving itself?
        mem_type: VideoFrameMemoryType,
        // Since we are using DMABUF, always allocate the maximum number of
        // V4L2 buffers (32) since they are virtually free. This gives more
        // flexibility for the client as to how many frames it can allocate.
        num_buffers: bindings::VIDEO_MAX_FRAME as usize,
    })
}

fn frame_decoded_cb(
    decoder: &mut v4l2r_decoder,
    mut dqbuf: DqBuffer<Capture, v4l2r_video_frame>,
    event_cb: v4l2r_decoder_event_cb,
    cb_data: *mut c_void,
) {
    let frame = dqbuf.take_handles().unwrap();
    debug!(
        "Video frame {} ({}) decoded from V4L2 buffer {} (flags: {:?})",
        frame.id,
        dqbuf.data.timestamp().tv_sec,
        dqbuf.data.index(),
        dqbuf.data.flags(),
    );
    let mut v4l2_data = dqbuf.data.clone();
    // Drop the DQBuffer early so the C callback can reuse the V4L2
    // buffer if it needs to.
    drop(dqbuf);

    // Immediately recycle empty frames. We will pass the corresponding
    // event to the client.
    if *v4l2_data.get_first_plane().bytesused == 0 {
        debug!(
            "Immediately recycling zero-sized frame {} {}",
            frame.id,
            v4l2_data.is_last()
        );
        // Should be safe as `provider` is initialized in the format
        // change callback and is thus valid, as well as `frame`.
        match &decoder.provider {
            Some(provider) => unsafe {
                v4l2r_video_frame_provider_queue_frame(provider.as_ref(), frame);
            },
            None => {
                error!("Frame decoded callback called while no provider set!");
            }
        }
    } else {
        // TODO check return value?
        event_cb(
            cb_data,
            &mut v4l2r_decoder_event::FrameDecoded(v4l2r_decoder_frame_decoded_event {
                buffer: v4l2_data.as_mut_ptr() as *const _,
                frame,
            }),
        );
    }
}

// A void pointer that can be sent across threads. This is usually not allowed
// by Rust, but is necessary for us to call back into the V4L2RustDecoder.
struct SendablePtr<T>(*mut T);
impl<T> Clone for SendablePtr<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T> Copy for SendablePtr<T> {}
unsafe impl<T> Send for SendablePtr<T> {}
unsafe impl<T> Sync for SendablePtr<T> {}

#[allow(clippy::too_many_arguments)]
fn v4l2r_decoder_new_safe(
    path: &Path,
    input_format_fourcc: u32,
    num_input_buffers: usize,
    input_buffer_size: usize,
    output_format_fourcc: u32,
    input_done_cb: v4l2r_decoder_input_done_cb,
    event_cb: v4l2r_decoder_event_cb,
    cb_data: *mut c_void,
) -> *mut v4l2r_decoder {
    let decoder = match Decoder::open(path) {
        Ok(decoder) => decoder,
        Err(e) => {
            error!("failed to open decoder {}: {:#?}", path.display(), e);
            return std::ptr::null_mut();
        }
    };

    info!(
        "Opened decoder {} with format {}, {} input buffers of size {}",
        path.display(),
        v4l2r::PixelFormat::from(input_format_fourcc),
        num_input_buffers,
        input_buffer_size
    );

    let format_builder = |f: FormatBuilder| {
        let pixel_format = input_format_fourcc.into();
        let format = match f
            .set_pixelformat(pixel_format)
            .set_planes_layout(vec![PlaneLayout {
                sizeimage: input_buffer_size as u32,
                ..Default::default()
            }])
            .apply::<v4l2r::Format>()
        {
            Ok(format) if format.pixelformat == pixel_format => format,
            Ok(_) => {
                return Err(anyhow::anyhow!(
                    "Unrecognized OUTPUT format {:?}",
                    pixel_format
                ))
            }
            Err(e) => return Err(e.into()),
        };
        debug!(
            "Decoder requires input buffer size of: {}",
            format.plane_fmt[0].sizeimage
        );
        Ok(())
    };
    let decoder = match decoder.set_output_format(format_builder) {
        Ok(decoder) => decoder,
        Err(e) => {
            error!("Error while setting output format: {}", e);
            return std::ptr::null_mut();
        }
    };

    let output_format = match output_format_fourcc {
        0 => None,
        fourcc => Some(PixelFormat::from(fourcc)),
    };

    let cb_data = SendablePtr(cb_data);

    let decoder =
        match decoder.allocate_output_buffers::<Vec<DmaBufHandle<DmaBufFd>>>(num_input_buffers) {
            Ok(decoder) => decoder,
            Err(e) => {
                error!("Error while allocating OUTPUT buffers: {}", e);
                return std::ptr::null_mut();
            }
        };

    // Reserve memory on the heap for our decoder and take a pointer that we
    // can use in our callbacks.
    let mut decoder_box = Box::new(MaybeUninit::<v4l2r_decoder>::uninit());
    let decoder_ptr = SendablePtr(decoder_box.as_mut_ptr());

    let event_handler = move |event: DecoderEvent<Arc<v4l2r_video_frame_provider>>| {
        let decoder = unsafe { decoder_ptr.0.as_mut().unwrap() };

        match event {
            DecoderEvent::FrameDecoded(dqbuf) => {
                frame_decoded_cb(decoder, dqbuf, event_cb, cb_data.0)
            }
            DecoderEvent::EndOfStream => event_cb(cb_data.0, &mut v4l2r_decoder_event::EndOfStream),
        };
    };

    let res = decoder.start(
        Box::new(
            move |buf: CompletedInputBuffer<Vec<DmaBufHandle<DmaBufFd>>>| {
                match buf {
                    CompletedInputBuffer::Dequeued(mut dqbuf) => {
                        debug!("Input buffer {} done", dqbuf.data.index());
                        // TODO check return value?
                        input_done_cb(cb_data.0, dqbuf.data.as_mut_ptr() as *const _);
                    }
                    // Just drop canceled buffers for now - the client will remove
                    // them on its side as well.
                    // TODO add a status parameter to the callback and invoke it?
                    // that way the client does not need to clear its own list...
                    CompletedInputBuffer::Canceled(_) => (),
                }
            },
        ) as Box<dyn InputDoneCallback<Vec<DmaBufHandle<DmaBufFd>>>>,
        Box::new(event_handler) as Box<dyn DecoderEventCallback<Arc<v4l2r_video_frame_provider>>>,
        Box::new(
            move |f: FormatBuilder,
                  visible_rect: Rect,
                  min_num_buffers: usize|
                  -> anyhow::Result<FormatChangedReply<Arc<v4l2r_video_frame_provider>>> {
                set_capture_format_cb(
                    f,
                    output_format,
                    visible_rect,
                    min_num_buffers,
                    decoder_ptr.0,
                    event_cb,
                    cb_data.0,
                )
            },
        ) as Box<dyn FormatChangedCallback<Arc<v4l2r_video_frame_provider>>>,
    );

    let decoder = match res {
        Ok(decoder) => decoder,
        Err(e) => {
            error!("Cannot start decoder: {}", e);
            return std::ptr::null_mut();
        }
    };

    let input_format: v4l2r::Format = decoder.get_output_format().unwrap();

    let decoder = v4l2r_decoder {
        decoder,
        provider: None,
        input_buf_size: input_format.plane_fmt[0].sizeimage as u64,
    };

    let decoder_box = unsafe {
        // Replace our uninitialized heap memory with our valid decoder.
        decoder_box.as_mut_ptr().write(decoder);
        // Convert the Box<MaybeUninit<v4l2r_decoder>> into Box<v4l2r_decoder>
        // now that we know the decoder is properly initialized. It would be
        // better to use Box::assume_init but as of rustc 1.50 this method is
        // still in nightly only.
        Box::from_raw(Box::into_raw(decoder_box) as *mut v4l2r_decoder)
    };

    info!("Decoder {:p}: successfully started", decoder_box.as_ref());

    Box::into_raw(decoder_box)
}

fn v4l2r_decoder_decode_safe(
    decoder: &mut v4l2r_decoder,
    bitstream_id: i32,
    fd: c_int,
    bytes_used: usize,
) -> c_int {
    let v4l2_buffer = match decoder.decoder.get_buffer() {
        Ok(buffer) => buffer,
        Err(e) => {
            error!("Error obtaining V4L2 buffer: {}", e);
            return -1;
        }
    };
    let v4l2_buffer_id = v4l2_buffer.index();

    match v4l2_buffer
        .set_timestamp(TimeVal::seconds(bitstream_id as i64))
        .queue_with_handles(
            vec![DmaBufHandle::from(DmaBufFd::new(
                fd,
                decoder.input_buf_size,
            ))],
            &[bytes_used],
        ) {
        Ok(()) => (),
        Err(e) => {
            error!("Error while queueing buffer: {}", e);
            return -1;
        }
    };

    v4l2_buffer_id as c_int
}

/// Create a new decoder for a given encoded format.
///
/// * `path` is the path to the V4L2 device that will be used for decoding.
/// * `input_format_fourcc` is the FOURCC code of the encoded format we will
///   decode, e.g. "H264" or "VP80".
/// * `num_input_buffers` is the number of input buffers we wish to use. It
///   should correspond to the number of different buffers containing input data
///   that will be given to this decoder.
/// * `input_buffer_size` is the desired size of input buffers. The decoder may
///   adjust this value, so the client should call
///   [`v4l2r_decoder_get_input_format`] to confirm the actual expected value.
/// * `output_format_fourcc` is the FOURCC code of the desired pixel format for
///   output frames (e.g. "NV12"). It can also be 0, in which case the decoder
///   will use whichever pixel format is active by default.
/// * `input_done_cb` is a pointer to a callback function to be called whenever
///   an encoded input buffer is done being processed. This callback is
///   guaranteed to be invoked during calls to [`v4l2r_decoder_decode`] or
///   [`v4l2r_decoder_kick`], i.e. it will always be called in the current
///   thread.
/// * `event_cb` is a pointer to a function to be called for handling the
///   various events produced by the decoder. See [`v4l2r_decoder_event`] for
///   more details on events. This callback is guaranteed to be called from a
///   separate, unique thread, therefore the events can be assumed to be
///   sequential (i.e. two events cannot be produced at the same time from two
///   different threads).
/// * `cb_data` is a pointer that will always be passed as the first parameter
///   of the `input_done_cb` and `events_cb`.
///
/// # Safety
/// The passed `path` must be a valid, zero-terminated C string containining the
/// path to the device. Expect a crash if passing an invalid string.
#[no_mangle]
pub unsafe extern "C" fn v4l2r_decoder_new(
    path: *const c_char,
    input_format_fourcc: u32,
    num_input_buffers: usize,
    input_buffer_size: usize,
    output_format_fourcc: u32,
    input_done_cb: v4l2r_decoder_input_done_cb,
    event_cb: v4l2r_decoder_event_cb,
    cb_data: *mut c_void,
) -> *mut v4l2r_decoder {
    let cstr = CStr::from_ptr(path);
    let rstr = cstr.to_str().unwrap();
    let path = Path::new(&rstr);

    v4l2r_decoder_new_safe(
        path,
        input_format_fourcc,
        num_input_buffers,
        input_buffer_size,
        output_format_fourcc,
        input_done_cb,
        event_cb,
        cb_data,
    )
}

/// Stop and destroy a decoder.
///
/// Stop `decoder` and destroy it. This function DOES take ownership of
/// `decoder`, which must absolutely not be used after this call.
///
/// It is guaranteed that none of the callbacks passed to [`v4l2r_decoder_new`]
/// will be called after this function has returned.
///
/// # Safety
///
/// `decoder` must be a valid pointer to a decoder returned by
/// `v4l2r_decoder_new`. Passing a NULL or invalid pointer will cause a crash.
/// `decoder` must not be used again after this function is called.
#[no_mangle]
pub unsafe extern "C" fn v4l2r_decoder_destroy(decoder: *mut v4l2r_decoder) {
    info!("Decoder {:p}: destroying", decoder);

    if decoder.is_null() {
        warn!("Trying to destroy a NULL decoder");
        return;
    }

    let decoder = Box::from_raw(decoder);
    match decoder.decoder.stop() {
        Ok(_) => (),
        Err(e) => error!("Error while stopping decoder: {}", e),
    }
}

/// Obtain the current input format (i.e. the format set on the *OUTPUT* queue).
///
/// Obtain the current input format for `decoder` and write it into `format`.
/// This function can be called at any time since a decoder always have a valid
/// input format.
///
/// Returns 0 in case of success, -1 if an error occured, in which case `format`
/// is not overwritten.
///
/// # Safety
///
/// `decoder` must be a valid pointer to a decoder instance. `format` must point
/// to valid memory that can receive a `v4l2_format`
#[no_mangle]
pub unsafe extern "C" fn v4l2r_decoder_get_input_format(
    decoder: *const v4l2r_decoder,
    format: *mut bindings::v4l2_format,
) -> c_int {
    assert!(!decoder.is_null());
    assert!(!format.is_null());

    let decoder = &*decoder;
    let format = &mut *format;

    *format = match decoder.decoder.get_output_format() {
        Ok(format) => format,
        Err(e) => {
            error!("Error while getting output format: {}", e);
            return -1;
        }
    };

    0
}

/// Decode the encoded data referenced by `fd`.
///
/// The decoder does NOT take ownership of `fd` and won't close it.
///
/// `bitstream_id` is the identifier of this input buffer. The produced frames
/// will carry this identifier in they timestamp.
///
/// `bytes_used` is amount of encoded data within that buffer.
///
/// The value returned is the index of the V4L2 buffer `fd` has been queued with.
/// It can be used to know when `fd` is done being decoded as a `v4l2_buffer` of
/// the same index will be passed as argument to the *input done callback* when
/// this is the case.
///
/// In case of error, -1 is returned.
///
/// # Safety
///
/// `decoder` must be a valid pointer to a decoder returned by
/// [`v4l2r_decoder_new`]. Passing a NULL or invalid pointer will cause a crash.
/// `fd` is expected to be a valid DMABUF FD backed by enough memory for the
/// expected input buffer size. Failure to provide a valid FD will return in an
/// ioctl error (but no crash).
#[no_mangle]
pub unsafe extern "C" fn v4l2r_decoder_decode(
    decoder: *mut v4l2r_decoder,
    bitstream_id: i32,
    fd: c_int,
    bytes_used: usize,
) -> c_int {
    debug!(
        "Decoder {:p}: decoding bitstream id {}",
        decoder, bitstream_id
    );
    assert!(!decoder.is_null());
    let decoder = &mut *decoder;

    v4l2r_decoder_decode_safe(decoder, bitstream_id, fd, bytes_used)
}

/// Kick the decoder and see if some input buffers fall as a result.
///
/// No, really. Completed input buffers are typically checked when calling
/// [`v4l2r_decoder_decode`] (which is also the time when the input done
/// callback is invoked), but this mechanism is not foolproof: if the client
/// works with a limited set of input buffers and queues them all before an
/// output frame can be produced, then the client has no more material to call
/// [`v4l2r_decoder_decode`] with and thus no input buffer will ever be
/// dequeued, resulting in the decoder being blocked.
///
/// This method mitigates this problem by adding a way to check for completed
/// input buffers and calling the input done callback without the need for new
/// encoded content. It is suggested to call it from the same thread that
/// invokes [`v4l2r_decoder_decode`] every time we get a
/// [`v4l2r_decoder_frame_decoded_event`]. That way the client can recycle its
/// input buffers and the decoding process does not get stuck.
///
/// # Safety
///
/// `decoder` must be a valid pointer to a decoder returned by
/// [`v4l2r_decoder_new`]. Passing a NULL or invalid pointer will cause a crash.
#[no_mangle]
pub unsafe extern "C" fn v4l2r_decoder_kick(decoder: *const v4l2r_decoder) {
    assert!(!decoder.is_null());
    let decoder = &*decoder;

    match decoder.decoder.kick() {
        Ok(()) => (),
        Err(e) => {
            error!("Error while kicking decoder: {}", e);
        }
    }
}

/// Possible responses for the [`v4l2r_decoder_drain`] commmand.
#[repr(C)]
#[allow(clippy::upper_case_acronyms)]
pub enum v4l2r_decoder_drain_response {
    /// The drain has already completed as [`v4l2r_decoder_drain`] returned.
    DRAIN_COMPLETED,
    /// The drain has started but will be completed when we receive a
    /// [`v4l2r_decoder_event::EndOfStream`] event.
    DRAIN_STARTED,
    /// Drain cannot be done at the moment because not enough input buffers
    /// have been processed to know the output format.
    TRY_AGAIN,
    /// An error has occurred.
    ERROR,
}

/// # Safety
///
/// `decoder` must be a valid pointer to a decoder returned by
/// [`v4l2r_decoder_new`]. Passing a NULL or invalid pointer will cause a crash.
#[no_mangle]
pub unsafe extern "C" fn v4l2r_decoder_drain(
    decoder: *const v4l2r_decoder,
    blocking: bool,
) -> v4l2r_decoder_drain_response {
    assert!(!decoder.is_null());
    let decoder = &*decoder;

    match decoder.decoder.drain(blocking) {
        Ok(true) => v4l2r_decoder_drain_response::DRAIN_COMPLETED,
        Ok(false) => v4l2r_decoder_drain_response::DRAIN_STARTED,
        Err(DrainError::TryAgain) => v4l2r_decoder_drain_response::TRY_AGAIN,
        Err(e) => {
            error!("Error while draining decoder: {}", e);
            v4l2r_decoder_drain_response::ERROR
        }
    }
}

/// # Safety
///
/// `decoder` must be a valid pointer to a decoder returned by
/// [`v4l2r_decoder_new`]. Passing a NULL or invalid pointer will cause a crash.
#[no_mangle]
pub unsafe extern "C" fn v4l2r_decoder_flush(decoder: *const v4l2r_decoder) {
    assert!(!decoder.is_null());
    let decoder = &*decoder;

    match decoder.decoder.flush() {
        Ok(()) => (),
        Err(e) => {
            error!("Error while flushing decoder: {:#?}", e);
        }
    }
}

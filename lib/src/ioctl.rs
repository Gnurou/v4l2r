//! This module provides safer versions of the V4L2 ioctls through simple functions working on a
//! `RawFd`, and safer variants of the main V4L2 structures. This module can be used directly, but
//! the `device` module is very likely to be a better fit for application code.
//!
//! V4L2 ioctls are usually called with a single structure as argument, which serves to store both
//! the input and output of the ioctl. This is quite error-prone as the user needs to remember
//! which parts of the structure they are supposed to fill, and which parts the driver will update.
//!
//! To alleviate this issue, this module tries to provide, for each ioctl:
//!
//! Consequently, each ioctl proxy function is designed as follows:
//!
//! * A function that takes the relevant input as parameters and not the entire input/output
//! structure. This lifts any ambiguity as to which parts of the structure userspace is supposed to
//! fill.
//! * Safe variants of V4L2 structures used in ioctls that can be build from their C counterparts
//! (and vice-versa) and include a validation step, to be used as return values.
//!
//! For instance, the `VIDIOC_G_FMT` ioctl takes a `struct v4l2_format` as argument, but only the
//! its `type` field is set by user-space - the rest of the structure is to be filled by the
//! driver.
//!
//! Therefore, our [`crate::ioctl::g_fmt()`] ioctl function takes the requested queue type as
//! argument and takes care of managing the `struct v4l2_format` to be passed to the kernel. The
//! filled structure is then converted into the type desired by the caller using
//! `TryFrom<v4l2_format>`:
//!
//! ```text
//! pub fn g_fmt<O: TryFrom<bindings::v4l2_format>>(
//!     fd: &impl AsRawFd,
//!     queue: QueueType,
//! ) -> Result<O, GFmtError>;
//! ```
//!
//! Since `struct v4l2_format` has C unions that are unsafe to use in Rust, the [`crate::Format`]
//! type can be used as the output type of this function, to validate the `struct v4l2_format`
//! returned by the kernel and convert it to a safe type.
//!
//! Most ioctls also have their own error type: this helps discern scenarios where the ioctl
//! returned non-zero, but the situation is not necessarily an error. For instance, `VIDIOC_DQBUF`
//! can return -EAGAIN if no buffer is available to dequeue, which is not an error and thus is
//! represented by its own variant. Actual errors are captured by the `IoctlError` variant, and all
//! error types can be converted to their original error code using their `Into<Errno>`
//! implementation.

mod decoder_cmd;
mod dqbuf;
mod encoder_cmd;
mod enum_fmt;
mod expbuf;
mod frameintervals;
mod framesizes;
mod g_audio;
mod g_dv_timings;
mod g_ext_ctrls;
mod g_fmt;
mod g_input;
mod g_jpegcomp;
mod g_parm;
mod g_selection;
mod mmap;
mod qbuf;
mod querybuf;
mod querycap;
mod queryctrl;
mod reqbufs;
mod request;
mod streamon;
mod subscribe_event;

pub use decoder_cmd::*;
pub use dqbuf::*;
pub use encoder_cmd::*;
pub use enum_fmt::*;
pub use expbuf::*;
pub use frameintervals::*;
pub use framesizes::*;
pub use g_audio::*;
pub use g_dv_timings::*;
pub use g_ext_ctrls::*;
pub use g_fmt::*;
pub use g_input::*;
pub use g_jpegcomp::*;
pub use g_parm::*;
pub use g_selection::*;
pub use mmap::*;
pub use qbuf::*;
pub use querybuf::*;
pub use querycap::*;
pub use queryctrl::*;
pub use reqbufs::*;
pub use request::*;
pub use streamon::*;
pub use subscribe_event::*;

use std::ffi::CStr;
use std::ffi::FromBytesWithNulError;
use std::fmt::Debug;
use std::ops::Deref;
use std::ops::DerefMut;

use bitflags::bitflags;
use enumn::N;
use nix::errno::Errno;

use crate::bindings;
use crate::memory::DmaBuf;
use crate::memory::Memory;
use crate::memory::MemoryType;
use crate::memory::Mmap;
use crate::memory::UserPtr;
use crate::QueueType;

/// Utility function for sub-modules.
/// Constructs an owned String instance from a slice containing a nul-terminated
/// C string, after checking that the passed slice indeed contains a nul
/// character.
fn string_from_cstr(c_str: &[u8]) -> Result<String, FromBytesWithNulError> {
    // Make sure that our string contains a nul character.
    let slice = match c_str.iter().position(|x| *x == b'\0') {
        // Pass the full slice, `from_bytes_with_nul` will return an error.
        None => c_str,
        Some(pos) => &c_str[..pos + 1],
    };

    Ok(CStr::from_bytes_with_nul(slice)?
        .to_string_lossy()
        .into_owned())
}

/// Returns whether the given queue type can handle multi-planar formats.
fn is_multi_planar(queue: QueueType) -> bool {
    matches!(
        queue,
        QueueType::VideoCaptureMplane | QueueType::VideoOutputMplane
    )
}

/// Extension trait for allowing easy conversion of ioctl errors into their originating error code.
pub trait IntoErrno {
    fn into_errno(self) -> i32;
}

impl<T> IntoErrno for T
where
    T: Into<Errno>,
{
    fn into_errno(self) -> i32 {
        self.into() as i32
    }
}

/// A memory area we can pass to ioctls in order to get/set plane information
/// with the multi-planar API.
type V4l2BufferPlanes = [bindings::v4l2_plane; bindings::VIDEO_MAX_PLANES as usize];

/// Information about a single plane of a V4L2 buffer.
#[repr(transparent)]
pub struct V4l2BufferPlane(bindings::v4l2_plane);

impl Debug for V4l2BufferPlane {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("V4l2BufferPlane")
            .field("length", &self.length())
            .field("bytesused", &self.bytesused())
            .field("data_offset", &self.data_offset())
            .finish()
    }
}

impl V4l2BufferPlane {
    pub fn length(&self) -> u32 {
        self.0.length
    }

    pub fn set_length(&mut self, length: u32) {
        self.0.length = length;
    }

    pub fn bytesused(&self) -> u32 {
        self.0.bytesused
    }

    pub fn set_bytesused(&mut self, bytesused: u32) {
        self.0.bytesused = bytesused;
    }

    pub fn data_offset(&self) -> u32 {
        self.0.data_offset
    }

    pub fn set_data_offset(&mut self, data_offset: u32) {
        self.0.data_offset = data_offset;
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug, Default)]
    /// `flags` member of `struct `v4l2_buffer`.
    pub struct BufferFlags: u32 {
        const MAPPED = bindings::V4L2_BUF_FLAG_MAPPED;
        const QUEUED = bindings::V4L2_BUF_FLAG_QUEUED;
        const DONE = bindings::V4L2_BUF_FLAG_DONE;
        const ERROR = bindings::V4L2_BUF_FLAG_ERROR;
        const KEYFRAME = bindings::V4L2_BUF_FLAG_KEYFRAME;
        const PFRAME = bindings::V4L2_BUF_FLAG_PFRAME;
        const BFRAME = bindings::V4L2_BUF_FLAG_BFRAME;
        const TIMECODE = bindings::V4L2_BUF_FLAG_TIMECODE;
        const PREPARED = bindings::V4L2_BUF_FLAG_PREPARED;
        const NO_CACHE_INVALIDATE = bindings::V4L2_BUF_FLAG_NO_CACHE_CLEAN;
        const NO_CACHE_CLEAN = bindings::V4L2_BUF_FLAG_NO_CACHE_INVALIDATE;
        const LAST = bindings::V4L2_BUF_FLAG_LAST;
        const TIMESTAMP_MONOTONIC = bindings::V4L2_BUF_FLAG_TIMESTAMP_MONOTONIC;
        const TIMESTAMP_COPY = bindings::V4L2_BUF_FLAG_TIMESTAMP_COPY;
        const TSTAMP_SRC_EOF = bindings::V4L2_BUF_FLAG_TSTAMP_SRC_EOF;
        const TSTAMP_SRC_SOE = bindings::V4L2_BUF_FLAG_TSTAMP_SRC_SOE;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, N)]
#[repr(u32)]
pub enum BufferField {
    Any = bindings::v4l2_field_V4L2_FIELD_ANY,
    None = bindings::v4l2_field_V4L2_FIELD_NONE,
    Top = bindings::v4l2_field_V4L2_FIELD_TOP,
    Interlaced = bindings::v4l2_field_V4L2_FIELD_INTERLACED,
    SeqTb = bindings::v4l2_field_V4L2_FIELD_SEQ_TB,
    SeqBt = bindings::v4l2_field_V4L2_FIELD_SEQ_BT,
    Alternate = bindings::v4l2_field_V4L2_FIELD_ALTERNATE,
    InterlacedTb = bindings::v4l2_field_V4L2_FIELD_INTERLACED_TB,
    InterlacedBt = bindings::v4l2_field_V4L2_FIELD_INTERLACED_BT,
}

/// Safe-ish representation of a `struct v4l2_buffer`. It owns its own planes array and can only be
/// constructed from valid data.
///
/// This structure guarantees the following invariants:
///
/// * The buffer's queue type is valid and cannot change,
/// * The buffer's memory type is valid and cannot change,
/// * The memory backing (MMAP offset/user pointer/DMABUF) can only be read and set according to
///   the memory type of the buffer. I.e. it is impossible to mistakenly set `fd` unless the
///   buffer's memory type is `DMABUF`.
///
/// Planes management is a bit complicated due to the existence of the single-planar and a
/// multi-planar buffer representations. There are situations where one wants to access plane
/// information regardless of the representation used, and others where one wants to access the
/// actual array of `struct v4l2_plane`, provided it exists.
///
/// For the first situation, use the `planes_iter` and `planes_iter_mut` methods. They return an
/// iterator to an accessor to plane data that is identical whether the buffer is single or multi
/// planar (or course, for single-planar buffers the length of the iterator will be exactly 1).
///
/// For the second situation, the `as_v4l2_planes` method returns an actual slice of `struct
/// v4l2_plane` with the plane information if the buffer is multi-planar (and an empty slice if the
/// it is single-planar).
#[derive(Clone)]
#[repr(C)]
pub struct V4l2Buffer {
    buffer: bindings::v4l2_buffer,
    planes: V4l2BufferPlanes,
}

/// V4l2Buffer is safe to send across threads. `v4l2_buffer` is !Send because it
/// contains a pointer, but we are making sure to use it safely here.
unsafe impl Send for V4l2Buffer {}

impl Debug for V4l2Buffer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("V4l2Buffer")
            .field("index", &self.index())
            .field("flags", &self.flags())
            .field("sequence", &self.sequence())
            .finish()
    }
}

impl V4l2Buffer {
    pub fn new(queue: QueueType, index: u32, memory: MemoryType) -> Self {
        Self {
            buffer: bindings::v4l2_buffer {
                index,
                type_: queue as u32,
                memory: memory as u32,
                // Make sure that a multiplanar buffer always has at least one plane.
                length: if queue.is_multiplanar() {
                    1
                } else {
                    Default::default()
                },
                ..Default::default()
            },
            planes: Default::default(),
        }
    }

    pub fn index(&self) -> u32 {
        self.buffer.index
    }

    pub fn queue(&self) -> QueueType {
        QueueType::n(self.buffer.type_).unwrap()
    }

    pub fn memory(&self) -> MemoryType {
        MemoryType::n(self.buffer.memory).unwrap()
    }

    /// Returns the currently set flags for this buffer.
    pub fn flags(&self) -> BufferFlags {
        BufferFlags::from_bits_truncate(self.buffer.flags)
    }

    /// Sets the flags of this buffer.
    pub fn set_flags(&mut self, flags: BufferFlags) {
        self.buffer.flags = flags.bits();
    }

    /// Add `flags` to the set of flags for this buffer.
    pub fn add_flags(&mut self, flags: BufferFlags) {
        self.set_flags(self.flags() | flags);
    }

    /// Remove `flags` from the set of flags for this buffer.
    pub fn clear_flags(&mut self, flags: BufferFlags) {
        self.set_flags(self.flags() - flags);
    }

    pub fn field(&self) -> BufferField {
        BufferField::n(self.buffer.field).unwrap()
    }

    pub fn set_field(&mut self, field: BufferField) {
        self.buffer.field = field as u32;
    }

    pub fn is_last(&self) -> bool {
        self.flags().contains(BufferFlags::LAST)
    }

    pub fn timestamp(&self) -> bindings::timeval {
        self.buffer.timestamp
    }

    pub fn set_timestamp(&mut self, timestamp: bindings::timeval) {
        self.buffer.timestamp = timestamp;
    }

    pub fn sequence(&self) -> u32 {
        self.buffer.sequence
    }

    pub fn set_sequence(&mut self, sequence: u32) {
        self.buffer.sequence = sequence;
    }

    pub fn num_planes(&self) -> usize {
        if self.queue().is_multiplanar() {
            self.buffer.length as usize
        } else {
            1
        }
    }

    /// Returns the first plane of the buffer. This method is guaranteed to
    /// succeed because every buffer has at least one plane.
    pub fn get_first_plane(&self) -> V4l2PlaneAccessor {
        self.planes_iter().next().unwrap()
    }

    /// Returns the first plane of the buffer. This method is guaranteed to
    /// succeed because every buffer has at least one plane.
    pub fn get_first_plane_mut(&mut self) -> V4l2PlaneMutAccessor {
        self.planes_iter_mut().next().unwrap()
    }

    /// Returns a non-mutable reference to the internal `v4l2_buffer`.
    ///
    /// The returned value is not suitable for passing to C functions or ioctls (which anyway
    /// require a mutable pointer), but can be useful to construct other values.
    ///
    /// In particular, if the buffer is multi-planar, then the `planes` pointer will be invalid.
    /// Dereferencing it would require an `unsafe` block anyway.
    ///
    /// If you need to access the `v4l2_planes` of this buffer, use `as_v4l2_planes`.
    ///
    /// If you need to pass the `v4l2_buffer` to a C function or ioctl and need a valid `planes`
    /// pointer, use `as_mut_ptr` and read the warning in its documentation.
    pub fn as_v4l2_buffer(&self) -> &bindings::v4l2_buffer {
        &self.buffer
    }

    /// Returns a slice of this buffer's `v4l2_plane`s, if the buffer is multi-planar.
    ///
    /// If it is single-planar, an empty slice is returned.
    ///
    /// This method only exists for the rare case when one needs to access the original plane data.
    /// For this reason there is no `v4l2_planes_mut` - use of the `planes_iter*_mut` methods
    /// instead if you need to modify plane information.
    pub fn as_v4l2_planes(&self) -> &[bindings::v4l2_plane] {
        let planes_upper = if self.queue().is_multiplanar() {
            self.buffer.length as usize
        } else {
            0
        };

        &self.planes[0..planes_upper]
    }

    /// Returns a pointer to the internal `v4l2_buffer`.
    ///
    /// If this buffer is multi-planar then the `planes` pointer will be updated so the returned
    /// data is valid if passed to a C function or an ioctl.
    ///
    /// Beware that as a consequence the returned pointer is only valid as long as the `V4l2Buffer`
    /// is not moved anywhere.
    ///
    /// Also, any unsafe code called on this pointer must maintain the invariants listed in
    /// [`V4l2Buffer`]'s documentation.
    ///
    /// Use with extreme caution.
    pub fn as_mut_ptr(&mut self) -> *mut bindings::v4l2_buffer {
        if self.queue().is_multiplanar() && self.buffer.length > 0 {
            self.buffer.m.planes = self.planes.as_mut_ptr();
        }

        &mut self.buffer as *mut _
    }

    /// Returns planar information in a way that is consistent between single-planar and
    /// multi-planar buffers.
    pub fn planes_iter(&self) -> impl Iterator<Item = V4l2PlaneAccessor> {
        let multiplanar = self.queue().is_multiplanar();
        let planes_iter = self.as_v4l2_planes().iter();

        // In order to return a consistent type for both single-planar and multi-planar buffers,
        // we chain the single-planar iterator to the multi-planar one. If the buffer is
        // single-planar, then the multi-planar iterator will be empty. If the buffer is
        // multi-planar, we skip the first entry which is the (invalid) single-planar iterator.
        std::iter::once(V4l2PlaneAccessor::new_single_planar(&self.buffer))
            .chain(planes_iter.map(V4l2PlaneAccessor::new_multi_planar))
            .skip(if multiplanar { 1 } else { 0 })
    }

    /// Returns planar information in a way that is consistent between single-planar and
    /// multi-planar buffers.
    pub fn planes_iter_mut(&mut self) -> impl Iterator<Item = V4l2PlaneMutAccessor> {
        let multiplanar = self.queue().is_multiplanar();
        let planes_upper = if multiplanar {
            self.buffer.length as usize
        } else {
            0
        };
        let planes_iter = self.planes[0..planes_upper].iter_mut();

        // In order to return a consistent type for both single-planar and multi-planar buffers,
        // we chain the single-planar iterator to the multi-planar one. If the buffer is
        // single-planar, then the multi-planar iterator will be empty. If the buffer is
        // multi-planar, we skip the first entry which is the (invalid) single-planar iterator.
        std::iter::once(V4l2PlaneMutAccessor::new_single_planar(&mut self.buffer))
            .chain(planes_iter.map(V4l2PlaneMutAccessor::new_multi_planar))
            .skip(if multiplanar { 1 } else { 0 })
    }

    /// Build a plane iterator including the memory backings for memory type `M`.
    ///
    /// # Safety
    ///
    /// The caller must be sure that the buffer's memory type is indeed `M`.
    unsafe fn planes_iter_with_backing<M: Memory>(
        &self,
    ) -> impl Iterator<Item = V4l2PlaneAccessorWithRawBacking<M>> {
        let is_multiplanar = self.queue().is_multiplanar();
        let planes_length = if is_multiplanar {
            self.buffer.length as usize
        } else {
            0
        };
        let planes = &self.planes[0..planes_length];
        // In order to return a consistent type for both single-planar and multi-planar buffers,
        // we chain the single-planar iterator to the multi-planar one. If the buffer is
        // single-planar, then the multi-planar iterator will be empty. If the buffer is
        // multi-planar, we skip the first entry which is the (invalid) single-planar iterator.
        std::iter::once(V4l2PlaneAccessorWithRawBacking::new_single_planar(
            &self.buffer,
        ))
        .chain(
            planes
                .iter()
                .map(|p| V4l2PlaneAccessorWithRawBacking::new_multi_planar(p)),
        )
        .skip(if self.queue().is_multiplanar() { 1 } else { 0 })
    }

    pub fn planes_with_backing_iter(
        &self,
    ) -> V4l2PlanesWithBacking<
        impl Iterator<Item = V4l2PlaneAccessorWithRawBacking<Mmap>>,
        impl Iterator<Item = V4l2PlaneAccessorWithRawBacking<UserPtr>>,
        impl Iterator<Item = V4l2PlaneAccessorWithRawBacking<DmaBuf>>,
    > {
        match self.memory() {
            MemoryType::Mmap => {
                V4l2PlanesWithBacking::Mmap(unsafe { self.planes_iter_with_backing() })
            }
            MemoryType::UserPtr => {
                V4l2PlanesWithBacking::UserPtr(unsafe { self.planes_iter_with_backing() })
            }
            MemoryType::DmaBuf => {
                V4l2PlanesWithBacking::DmaBuf(unsafe { self.planes_iter_with_backing() })
            }
            MemoryType::Overlay => V4l2PlanesWithBacking::Overlay,
        }
    }

    /// Build a mutable plane iterator including the memory backings for memory type `M`.
    ///
    /// # Safety
    ///
    /// The caller must be sure that the buffer's memory type is indeed `M`.
    unsafe fn planes_iter_with_backing_mut<M: Memory>(
        &mut self,
    ) -> impl Iterator<Item = V4l2PlaneMutAccessorWithRawBacking<M>> {
        let is_multiplanar = self.queue().is_multiplanar();
        let planes_length = if is_multiplanar {
            self.buffer.length as usize
        } else {
            0
        };
        let planes = &mut self.planes[0..planes_length];

        // In order to return a consistent type for both single-planar and multi-planar buffers,
        // we chain the single-planar iterator to the multi-planar one. If the buffer is
        // single-planar, then the multi-planar iterator will be empty. If the buffer is
        // multi-planar, we skip the first entry which is the (invalid) single-planar iterator.
        std::iter::once(V4l2PlaneMutAccessorWithRawBacking::new_single_planar(
            &mut self.buffer,
        ))
        .chain(
            planes
                .iter_mut()
                .map(|p| V4l2PlaneMutAccessorWithRawBacking::new_multi_planar(p)),
        )
        .skip(if is_multiplanar { 1 } else { 0 })
    }

    pub fn planes_with_backing_iter_mut(
        &mut self,
    ) -> V4l2PlanesWithBackingMut<
        impl Iterator<Item = V4l2PlaneMutAccessorWithRawBacking<Mmap>>,
        impl Iterator<Item = V4l2PlaneMutAccessorWithRawBacking<UserPtr>>,
        impl Iterator<Item = V4l2PlaneMutAccessorWithRawBacking<DmaBuf>>,
    > {
        match self.memory() {
            MemoryType::Mmap => {
                V4l2PlanesWithBackingMut::Mmap(unsafe { self.planes_iter_with_backing_mut() })
            }
            MemoryType::UserPtr => {
                V4l2PlanesWithBackingMut::UserPtr(unsafe { self.planes_iter_with_backing_mut() })
            }
            MemoryType::DmaBuf => {
                V4l2PlanesWithBackingMut::DmaBuf(unsafe { self.planes_iter_with_backing_mut() })
            }
            MemoryType::Overlay => V4l2PlanesWithBackingMut::Overlay,
        }
    }
}

/// Accessor to a buffer's plane information.
///
/// This is just a set of references, that are set to point to the right location depending on
/// whether the buffer is single or multi-planar.
pub struct V4l2PlaneAccessor<'a> {
    pub bytesused: &'a u32,
    pub length: &'a u32,
    pub data_offset: Option<&'a u32>,
}

impl<'a> V4l2PlaneAccessor<'a> {
    fn new_single_planar(buffer: &'a bindings::v4l2_buffer) -> Self {
        Self {
            bytesused: &buffer.bytesused,
            length: &buffer.length,
            data_offset: None,
        }
    }

    fn new_multi_planar(plane: &'a bindings::v4l2_plane) -> Self {
        Self {
            bytesused: &plane.bytesused,
            length: &plane.length,
            data_offset: Some(&plane.data_offset),
        }
    }
}

/// Mutable accessor to a buffer's plane information.
///
/// This is just a set of references, that are set to point to the right location depending on
/// whether the buffer is single or multi-planar.
pub struct V4l2PlaneMutAccessor<'a> {
    pub bytesused: &'a mut u32,
    pub length: &'a mut u32,
    pub data_offset: Option<&'a mut u32>,
}

impl<'a> V4l2PlaneMutAccessor<'a> {
    fn new_single_planar(buffer: &'a mut bindings::v4l2_buffer) -> Self {
        Self {
            bytesused: &mut buffer.bytesused,
            length: &mut buffer.length,
            data_offset: None,
        }
    }

    fn new_multi_planar(plane: &'a mut bindings::v4l2_plane) -> Self {
        Self {
            bytesused: &mut plane.bytesused,
            length: &mut plane.length,
            data_offset: Some(&mut plane.data_offset),
        }
    }
}

pub struct V4l2PlaneAccessorWithRawBacking<'a, M: Memory> {
    data: V4l2PlaneAccessor<'a>,
    backing: &'a M::RawBacking,
}

impl<'a, M: Memory> Deref for V4l2PlaneAccessorWithRawBacking<'a, M> {
    type Target = V4l2PlaneAccessor<'a>;

    fn deref(&self) -> &Self::Target {
        &self.data
    }
}

impl<'a, M: Memory> V4l2PlaneAccessorWithRawBacking<'a, M> {
    /// Create a new plane accessor for memory type `M`.
    ///
    /// # Safety
    ///
    /// `v4l2_buffer` must be of a single-planar type and use memory type `M`.
    pub unsafe fn new_single_planar(buffer: &'a bindings::v4l2_buffer) -> Self {
        Self {
            data: V4l2PlaneAccessor::new_single_planar(buffer),
            backing: M::get_single_planar_buffer_backing(&buffer.m),
        }
    }

    /// Create a new plane accessor for memory type `M`.
    ///
    /// # Safety
    ///
    /// `v4l2_plane` must come from a multi-planar buffer using memory type `M`.
    pub unsafe fn new_multi_planar(plane: &'a bindings::v4l2_plane) -> Self {
        Self {
            data: V4l2PlaneAccessor::new_multi_planar(plane),
            backing: M::get_plane_buffer_backing(&plane.m),
        }
    }
}

impl<'a> V4l2PlaneAccessorWithRawBacking<'a, Mmap> {
    pub fn mem_offset(&self) -> <Mmap as Memory>::RawBacking {
        *self.backing
    }
}

impl<'a> V4l2PlaneAccessorWithRawBacking<'a, UserPtr> {
    pub fn userptr(&self) -> <UserPtr as Memory>::RawBacking {
        *self.backing
    }
}

impl<'a> V4l2PlaneAccessorWithRawBacking<'a, DmaBuf> {
    pub fn fd(&self) -> <DmaBuf as Memory>::RawBacking {
        *self.backing
    }
}

pub enum V4l2PlanesWithBacking<
    'a,
    M: Iterator<Item = V4l2PlaneAccessorWithRawBacking<'a, Mmap>>,
    U: Iterator<Item = V4l2PlaneAccessorWithRawBacking<'a, UserPtr>>,
    D: Iterator<Item = V4l2PlaneAccessorWithRawBacking<'a, DmaBuf>>,
> {
    Mmap(M),
    UserPtr(U),
    DmaBuf(D),
    Overlay,
}

pub struct V4l2PlaneMutAccessorWithRawBacking<'a, M: Memory> {
    data: V4l2PlaneMutAccessor<'a>,
    backing: &'a mut M::RawBacking,
}

impl<'a, M: Memory> Deref for V4l2PlaneMutAccessorWithRawBacking<'a, M> {
    type Target = V4l2PlaneMutAccessor<'a>;

    fn deref(&self) -> &Self::Target {
        &self.data
    }
}

impl<'a, M: Memory> DerefMut for V4l2PlaneMutAccessorWithRawBacking<'a, M> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.data
    }
}

impl<'a, M: Memory> V4l2PlaneMutAccessorWithRawBacking<'a, M> {
    /// Create a new plane accessor for memory type `M`.
    ///
    /// # Safety
    ///
    /// `v4l2_buffer` must be of a single-planar type and use memory type `M`.
    pub unsafe fn new_single_planar(buffer: &'a mut bindings::v4l2_buffer) -> Self {
        Self {
            data: V4l2PlaneMutAccessor {
                bytesused: &mut buffer.bytesused,
                length: &mut buffer.length,
                data_offset: None,
            },
            backing: M::get_single_planar_buffer_backing_mut(&mut buffer.m),
        }
    }

    /// Create a new plane accessor for memory type `M`.
    ///
    /// # Safety
    ///
    /// `v4l2_plane` must come from a multi-planar buffer using memory type `M`.
    pub unsafe fn new_multi_planar(plane: &'a mut bindings::v4l2_plane) -> Self {
        Self {
            data: V4l2PlaneMutAccessor {
                bytesused: &mut plane.bytesused,
                length: &mut plane.length,
                data_offset: Some(&mut plane.data_offset),
            },
            backing: M::get_plane_buffer_backing_mut(&mut plane.m),
        }
    }
}

impl<'a> V4l2PlaneMutAccessorWithRawBacking<'a, Mmap> {
    pub fn mem_offset(&self) -> <Mmap as Memory>::RawBacking {
        *self.backing
    }

    pub fn set_mem_offset(&mut self, mem_offset: <Mmap as Memory>::RawBacking) {
        *self.backing = mem_offset;
    }
}

impl<'a> V4l2PlaneMutAccessorWithRawBacking<'a, UserPtr> {
    pub fn userptr(&self) -> <UserPtr as Memory>::RawBacking {
        *self.backing
    }

    pub fn set_userptr(&mut self, userptr: <UserPtr as Memory>::RawBacking) {
        *self.backing = userptr;
    }
}

impl<'a> V4l2PlaneMutAccessorWithRawBacking<'a, DmaBuf> {
    pub fn fd(&self) -> <DmaBuf as Memory>::RawBacking {
        *self.backing
    }

    pub fn set_fd(&mut self, fd: <DmaBuf as Memory>::RawBacking) {
        *self.backing = fd;
    }
}

pub enum V4l2PlanesWithBackingMut<
    'a,
    M: Iterator<Item = V4l2PlaneMutAccessorWithRawBacking<'a, Mmap>>,
    U: Iterator<Item = V4l2PlaneMutAccessorWithRawBacking<'a, UserPtr>>,
    D: Iterator<Item = V4l2PlaneMutAccessorWithRawBacking<'a, DmaBuf>>,
> {
    Mmap(M),
    UserPtr(U),
    DmaBuf(D),
    Overlay,
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_string_from_cstr() {
        use super::string_from_cstr;

        // Nul-terminated slice.
        assert_eq!(string_from_cstr(b"Hello\0"), Ok(String::from("Hello")));

        // Slice with nul in the middle and not nul-terminated.
        assert_eq!(string_from_cstr(b"Hi\0lo"), Ok(String::from("Hi")));

        // Slice with nul in the middle and nul-terminated.
        assert_eq!(string_from_cstr(b"Hi\0lo\0"), Ok(String::from("Hi")));

        // Slice starting with nul.
        assert_eq!(string_from_cstr(b"\0ello"), Ok(String::from("")));

        // Slice without nul.
        match string_from_cstr(b"Hello") {
            Err(_) => {}
            Ok(_) => panic!(),
        };

        // Empty slice.
        match string_from_cstr(b"") {
            Err(_) => {}
            Ok(_) => panic!(),
        };
    }
}

//! Safe definitions around V4L2 extended controls.
//!
//! Extended controls are represented using the `[SafeExtControl]` type, which is a transparent
//! wrapper for `v4l2_ext_control`. It takes a generic parameter defining the actual control to
//! use, which limits its API to the methods safe to use for that control type.
//!
//! A mutable reference to a single `[SafeExtControl]` can be passed to ioctl methods such as
//! [`crate::ioctl::g_ext_ctrls`] to get or set the value of that control only. Setting more than
//! one control at the same time requires to pass a type implementing [`AsV4l2ControlSlice`], that
//! returns a slice if the `v4l2_ext_control`s to manipulate.
//!
//! Since [`SafeExtControl`] is a transparent wrapper around `v4l2_ext_control`, an array of it can
//! safely implement `AsV4l2ControlSlice`. Or, more conveniently, a `#[repr(C)]` type containing
//! only [`SafeExtControl`]s:
//!
//! ```no_run
//! # use std::os::fd::OwnedFd;
//! # use std::path::Path;
//! #
//! # use v4l2r::bindings::v4l2_ext_control;
//! # use v4l2r::controls::AsV4l2ControlSlice;
//! # use v4l2r::controls::SafeExtControl;
//! # use v4l2r::controls::user::Brightness;
//! # use v4l2r::controls::user::Contrast;
//! # use v4l2r::device::Device;
//! # use v4l2r::ioctl::s_ext_ctrls;
//! #
//! # let device = Device::open(Path::new("/dev/video0"), Default::default()).unwrap();
//! #
//! #[repr(C)]
//! struct Controls {
//!     brightness: SafeExtControl<Brightness>,
//!     contrast: SafeExtControl<Contrast>,
//! }
//!
//! impl AsV4l2ControlSlice for &mut Controls {
//!     fn as_v4l2_control_slice(&mut self) -> &mut [v4l2_ext_control] {
//!         let ptr = (*self) as *mut Controls as *mut v4l2_ext_control;
//!         unsafe { std::slice::from_raw_parts_mut(ptr, 2) }
//!     }
//! }
//!
//! let mut controls = Controls {
//!     brightness: SafeExtControl::<Brightness>::from_value(128),
//!     contrast: SafeExtControl::<Contrast>::from_value(128),
//! };
//!
//! s_ext_ctrls::<OwnedFd, _>(&device, None, &mut controls).unwrap();
//! assert_eq!(controls.brightness.value(), 128);
//! assert_eq!(controls.contrast.value(), 128);
//! ```
//!
//! Due to the use of `repr(C)`, the `Controls` type has the same layout as an array of
//! `v4l2_ext_control`s and thus can be passed to `s_ext_ctrls` safely.
//!
//! Sub-modules contain the type definitions for each control, organized by control class. Due to
//! the large number of controls they are not all defined, so please add those you need if they are
//! missing.

pub mod codec;
pub mod user;

use std::marker::PhantomData;

use crate::bindings;
use crate::bindings::v4l2_ctrl_fwht_params;
use crate::bindings::v4l2_ctrl_vp8_frame;
use crate::bindings::v4l2_ext_control;
use crate::bindings::v4l2_ext_control__bindgen_ty_1;
use crate::controls::codec::FwhtFlags;

/// Trait implemented by types that can be passed to the
/// [`g/s/try_ext_ctrls`](crate::ioctl::g_ext_ctrls) family of functions.
pub trait AsV4l2ControlSlice {
    fn as_v4l2_control_slice(&mut self) -> &mut [v4l2_ext_control];
}

impl AsV4l2ControlSlice for &mut [v4l2_ext_control] {
    fn as_v4l2_control_slice(&mut self) -> &mut [v4l2_ext_control] {
        self
    }
}

/// Trait implemented by types representing a given control in order to define its properties and
/// set of available methods.
pub trait ExtControlTrait {
    /// One of `V4L2_CID_*`
    const ID: u32;
    /// Type of the value of this control.
    type PAYLOAD;
}

/// Memory-safe `v4l2_ext_control`.
///
/// This type is a `v4l2_ext_control` with the following invariants:
///
/// * `id` is always a valid control ID,
/// * `size` is always correct (0 for non-pointer controls or size of payload for pointer
///   controls),
/// * For pointer types, the payload is always allocated to match `size` bytes.
///
/// In addition, the value of the control can only be accessed through methods that return the
/// correct type.
#[repr(transparent)]
pub struct SafeExtControl<T: ExtControlTrait>(v4l2_ext_control, PhantomData<T>);

impl<T: ExtControlTrait> SafeExtControl<T> {
    pub fn id(&self) -> u32 {
        self.0.id
    }
}

/// Allows us to pass a `&mut` of a single `SafeExtControl` to `g/s/try_ext_ctrls`.
impl<T: ExtControlTrait> AsV4l2ControlSlice for &mut SafeExtControl<T> {
    fn as_v4l2_control_slice(&mut self) -> &mut [v4l2_ext_control] {
        unsafe { std::slice::from_raw_parts_mut(&mut self.0, 1) }
    }
}

impl<T> SafeExtControl<T>
where
    T: ExtControlTrait<PAYLOAD = i32>,
{
    /// Create a new control from its value.
    pub fn from_value(value: i32) -> Self {
        Self(
            v4l2_ext_control {
                id: T::ID,
                __bindgen_anon_1: v4l2_ext_control__bindgen_ty_1 { value },
                ..unsafe { std::mem::zeroed() }
            },
            PhantomData,
        )
    }

    /// Returns the value of the control.
    pub fn value(&self) -> i32 {
        unsafe { self.0.__bindgen_anon_1.value }
    }

    /// Updates the value of the control.
    pub fn set_value(&mut self, value: i32) {
        self.0.__bindgen_anon_1.value = value;
    }
}

impl<T> SafeExtControl<T>
where
    T: ExtControlTrait<PAYLOAD = i64>,
{
    /// Create a new control from its value.
    pub fn from_value64(value64: i64) -> Self {
        Self(
            v4l2_ext_control {
                id: T::ID,
                __bindgen_anon_1: v4l2_ext_control__bindgen_ty_1 { value64 },
                ..unsafe { std::mem::zeroed() }
            },
            PhantomData,
        )
    }

    /// Returns the value of the control.
    pub fn value64(&self) -> i64 {
        unsafe { self.0.__bindgen_anon_1.value64 }
    }

    /// Updates the value of the control.
    pub fn set_value64(&mut self, value: i64) {
        self.0.__bindgen_anon_1.value64 = value;
    }
}

impl<T> From<v4l2_ctrl_fwht_params> for SafeExtControl<T>
where
    T: ExtControlTrait<PAYLOAD = v4l2_ctrl_fwht_params>,
{
    fn from(params: v4l2_ctrl_fwht_params) -> Self {
        let payload = Box::new(params);

        Self(
            v4l2_ext_control {
                id: T::ID,
                size: std::mem::size_of::<T::PAYLOAD>() as u32,
                __bindgen_anon_1: v4l2_ext_control__bindgen_ty_1 {
                    p_fwht_params: Box::into_raw(payload),
                },
                ..unsafe { std::mem::zeroed() }
            },
            PhantomData,
        )
    }
}

impl<T> SafeExtControl<T>
where
    T: ExtControlTrait<PAYLOAD = v4l2_ctrl_fwht_params>,
{
    pub fn fwht_params(&self) -> &v4l2_ctrl_fwht_params {
        unsafe { self.0.__bindgen_anon_1.p_fwht_params.as_ref().unwrap() }
    }

    pub fn fwht_params_mut(&mut self) -> &mut v4l2_ctrl_fwht_params {
        unsafe { self.0.__bindgen_anon_1.p_fwht_params.as_mut().unwrap() }
    }

    pub fn flags(&self) -> Option<FwhtFlags> {
        FwhtFlags::from_bits(self.fwht_params().flags)
    }
}

impl<T> From<v4l2_ctrl_vp8_frame> for SafeExtControl<T>
where
    T: ExtControlTrait<PAYLOAD = v4l2_ctrl_vp8_frame>,
{
    fn from(params: v4l2_ctrl_vp8_frame) -> Self {
        let payload = Box::new(params);

        Self(
            v4l2_ext_control {
                id: T::ID,
                size: std::mem::size_of::<T::PAYLOAD>() as u32,
                __bindgen_anon_1: v4l2_ext_control__bindgen_ty_1 {
                    p_vp8_frame: Box::into_raw(payload),
                },
                ..unsafe { std::mem::zeroed() }
            },
            PhantomData,
        )
    }
}

impl<T> SafeExtControl<T>
where
    T: ExtControlTrait<PAYLOAD = v4l2_ctrl_vp8_frame>,
{
    pub fn vp8_frame(&self) -> &v4l2_ctrl_vp8_frame {
        unsafe { self.0.__bindgen_anon_1.p_vp8_frame.as_ref().unwrap() }
    }

    pub fn vp8_frame_mut(&mut self) -> &mut v4l2_ctrl_vp8_frame {
        unsafe { self.0.__bindgen_anon_1.p_vp8_frame.as_mut().unwrap() }
    }
}

// Due to a limitation of the type system we cannot conditionally implement the `Drop` trait on
// e.g. `where T: ControlTrait<PAYLOAD = v4l2_ctrl_fwht_params>`, so we need this global implementation.
impl<T: ExtControlTrait> Drop for SafeExtControl<T> {
    fn drop(&mut self) {
        // If we have allocated some payload for this control, re-wrap it into its original
        // container that we immediately drop to free it.
        if self.0.size > 0 {
            unsafe {
                match self.0.id {
                    bindings::V4L2_CID_STATELESS_FWHT_PARAMS => {
                        let _ = Box::from_raw(self.0.__bindgen_anon_1.p_fwht_params);
                    }
                    bindings::V4L2_CID_STATELESS_VP8_FRAME => {
                        let _ = Box::from_raw(self.0.__bindgen_anon_1.p_vp8_frame);
                    }
                    _ => (),
                }
            }
        }
    }
}

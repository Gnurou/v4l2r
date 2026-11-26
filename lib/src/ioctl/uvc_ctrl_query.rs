//! Safe wrapper for the `UVCIOC_CTRL_QUERY` ioctl of the Linux `uvcvideo` driver, which sends a
//! request to a control of a UVC extension unit and returns its data.
use std::os::unix::io::AsRawFd;

use nix::errno::Errno;
use thiserror::Error;

use crate::bindings;
use crate::bindings::uvc_xu_control_query;

#[doc(hidden)]
mod ioctl {
    use crate::bindings::uvc_xu_control_query;

    nix::ioctl_readwrite!(uvcioc_ctrl_query, b'u', 0x21, uvc_xu_control_query);
}

/// A request to a control of a UVC extension unit, as the USB Video Class specification names
/// them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UvcRequest {
    /// Sets the control's current value.
    SetCur,
    /// Reads the control's current value.
    GetCur,
    /// Reads the control's minimum value.
    GetMin,
    /// Reads the control's maximum value.
    GetMax,
    /// Reads the control's resolution.
    GetRes,
    /// Reads the length of the control's data, as two little-endian bytes.
    GetLen,
    /// Reads the control's capabilities and status, as one byte.
    GetInfo,
    /// Reads the control's default value.
    GetDef,
}

impl UvcRequest {
    /// The request's code, as `linux/usb/video.h` defines it.
    fn code(self) -> u8 {
        let code = match self {
            UvcRequest::SetCur => bindings::UVC_SET_CUR,
            UvcRequest::GetCur => bindings::UVC_GET_CUR,
            UvcRequest::GetMin => bindings::UVC_GET_MIN,
            UvcRequest::GetMax => bindings::UVC_GET_MAX,
            UvcRequest::GetRes => bindings::UVC_GET_RES,
            UvcRequest::GetLen => bindings::UVC_GET_LEN,
            UvcRequest::GetInfo => bindings::UVC_GET_INFO,
            UvcRequest::GetDef => bindings::UVC_GET_DEF,
        };
        code as u8
    }
}

#[derive(Debug, Error)]
pub enum UvcCtrlQueryError {
    #[error(
        "the control's data is {0} bytes, more than the {max} the driver takes",
        max = u16::MAX
    )]
    DataTooLong(usize),
    #[error("ioctl error: {0}")]
    IoctlError(Errno),
}

impl From<UvcCtrlQueryError> for Errno {
    fn from(err: UvcCtrlQueryError) -> Self {
        match err {
            UvcCtrlQueryError::DataTooLong(_) => Errno::EINVAL,
            UvcCtrlQueryError::IoctlError(e) => e,
        }
    }
}

/// Safe wrapper around the `UVCIOC_CTRL_QUERY` ioctl.
///
/// Sends `request` to control `selector` of extension unit `unit`. `data` carries the control's
/// data: the value to set for [`UvcRequest::SetCur`], or room for what a get request reads back,
/// whose length the driver checks against the control's.
pub fn uvc_ctrl_query(
    fd: &impl AsRawFd,
    unit: u8,
    selector: u8,
    request: UvcRequest,
    data: &mut [u8],
) -> Result<(), UvcCtrlQueryError> {
    let size = u16::try_from(data.len()).map_err(|_| UvcCtrlQueryError::DataTooLong(data.len()))?;
    let mut query = uvc_xu_control_query {
        unit,
        selector,
        query: request.code(),
        size,
        data: data.as_mut_ptr(),
    };

    // SAFETY: `query.data` points to the `size` bytes of `data`, which is borrowed exclusively
    // for the duration of the call, so the driver reads and writes only memory it is given.
    match unsafe { ioctl::uvcioc_ctrl_query(fd.as_raw_fd(), &mut query) } {
        Ok(_) => Ok(()),
        Err(e) => Err(UvcCtrlQueryError::IoctlError(e)),
    }
}

#[cfg(test)]
mod tests {
    use std::fs::File;

    use super::*;

    #[test]
    fn the_request_number_is_the_kernel_headers_own() {
        let ours =
            nix::request_code_readwrite!(b'u', 0x21, std::mem::size_of::<uvc_xu_control_query>());
        assert_eq!(ours as u64, bindings::UVCIOC_CTRL_QUERY);
    }

    #[test]
    fn the_request_codes_are_the_specifications() {
        // USB Video Class 1.5, Table A-8: SET_CUR is 0x01, the gets 0x81 to 0x87.
        assert_eq!(UvcRequest::SetCur.code(), 0x01);
        assert_eq!(UvcRequest::GetCur.code(), 0x81);
        assert_eq!(UvcRequest::GetLen.code(), 0x85);
        assert_eq!(UvcRequest::GetDef.code(), 0x87);
    }

    #[test]
    fn data_longer_than_the_driver_takes_is_refused_before_the_call() {
        let file = File::open("/dev/null").unwrap();
        let mut data = vec![0u8; usize::from(u16::MAX) + 1];
        let result = uvc_ctrl_query(&file, 4, 1, UvcRequest::GetCur, &mut data);
        assert!(matches!(result, Err(UvcCtrlQueryError::DataTooLong(_))));
    }

    #[test]
    fn a_device_without_the_driver_answers_with_an_ioctl_error() {
        let file = File::open("/dev/null").unwrap();
        let mut data = [0u8; 2];
        let result = uvc_ctrl_query(&file, 4, 1, UvcRequest::GetLen, &mut data);
        assert!(matches!(
            result,
            Err(UvcCtrlQueryError::IoctlError(Errno::ENOTTY))
        ));
    }
}

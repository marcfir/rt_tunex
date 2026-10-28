use std::{
    ffi::c_int,
    io::{Error, ErrorKind},
};

use libc::{
    kern_return_t, KERN_INVALID_ADDRESS, KERN_INVALID_ARGUMENT, KERN_INVALID_VALUE, KERN_NODE_DOWN,
    KERN_NOT_SUPPORTED, KERN_NO_ACCESS, KERN_NO_SPACE, KERN_OPERATION_TIMED_OUT,
    KERN_PROTECTION_FAILURE, KERN_RESOURCE_SHORTAGE, KERN_SUCCESS,
};

pub fn mach_error_to_io(kr: kern_return_t) -> std::io::Error {
    let kind = match kr {
        KERN_SUCCESS => return Error::from_raw_os_error(0),

        KERN_INVALID_ARGUMENT | KERN_INVALID_ADDRESS | KERN_INVALID_VALUE => {
            ErrorKind::InvalidInput
        }

        KERN_PROTECTION_FAILURE | KERN_NO_ACCESS | 53 => ErrorKind::PermissionDenied,

        KERN_RESOURCE_SHORTAGE => ErrorKind::OutOfMemory,

        KERN_NO_SPACE => ErrorKind::StorageFull,

        KERN_OPERATION_TIMED_OUT => ErrorKind::TimedOut,

        KERN_NODE_DOWN => ErrorKind::HostUnreachable,

        KERN_NOT_SUPPORTED => ErrorKind::Unsupported,

        _ => ErrorKind::Other,
    };

    Error::new(kind, format!("Mach error {}", kr))
}

pub fn mach_to_result<T>(ret: c_int, ok: T) -> Result<T, std::io::Error> {
    if ret == 0 {
        Ok(ok)
    } else {
        Err(mach_error_to_io(ret))
    }
}

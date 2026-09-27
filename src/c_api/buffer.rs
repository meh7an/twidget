//! The caller's memory: input slices, the environment, and output buffers with their lengths,
//! under the header's pointer rules.

use core::{mem, ptr, slice};

use super::TWidgetEnv;
use crate::expr::Env;
use crate::{Error, Result};

/// The caller's input bytes. A NULL pointer with length 0 is an empty input.
///
/// # Safety
/// A non-NULL `ptr` must point to `len` readable bytes that stay unchanged during the call.
pub(super) unsafe fn input<'a>(ptr: *const u8, len: usize) -> Result<&'a [u8]> {
    if ptr.is_null() {
        return if len == 0 {
            Ok(&[])
        } else {
            Err(Error::InvalidArgument {
                reason: "an input pointer is NULL but its length is not 0",
            })
        };
    }
    // No real buffer is this long, and `from_raw_parts` requires the length to fit an isize.
    if len > isize::MAX.unsigned_abs() {
        return Err(Error::InvalidArgument {
            reason: "an input length exceeds isize::MAX",
        });
    }
    // SAFETY: `ptr` is not NULL, the caller vouches for `len` readable bytes there that stay
    // unchanged, and `len` fits an isize.
    Ok(unsafe { slice::from_raw_parts(ptr, len) })
}

/// The environment the caller passes, read only after its `struct_size` shows that the caller's
/// struct holds every field of this header's.
///
/// # Safety
/// A non-NULL `env` must point to an aligned `TWidgetEnv` of at least `struct_size` readable
/// bytes, as the caller's version of the header defines it.
pub(super) unsafe fn env(env: *const TWidgetEnv) -> Result<Env> {
    if env.is_null() {
        return Err(Error::InvalidArgument {
            reason: "env is NULL",
        });
    }
    // SAFETY: every version of the header starts `TWidgetEnv` with `struct_size`, so the field is
    // there to read before the rest of the struct is known to be.
    let size = unsafe { ptr::addr_of!((*env).struct_size).read() };
    if !usize::try_from(size).is_ok_and(|size| size >= mem::size_of::<TWidgetEnv>()) {
        return Err(Error::InvalidArgument {
            reason: "env->struct_size is smaller than this library's sizeof(TWidgetEnv)",
        });
    }
    // SAFETY: `struct_size` shows the caller's struct is at least as large as this header's, and
    // the caller vouches for that many readable bytes.
    let env = unsafe { &*env };
    Ok(Env {
        now_ms: env.now_ms,
        tz_offset_minutes: env.tz_offset_minutes,
        dark: env.dark != 0,
        rtl: env.rtl != 0,
    })
}

/// Writes `value` through an optional output pointer, as `status_out`, `version_out` and
/// `next_wakeup_ms` are written.
///
/// # Safety
/// `slot` must be NULL or point to a writable, aligned `T`.
pub(super) unsafe fn put<T>(slot: *mut T, value: T) {
    if !slot.is_null() {
        // SAFETY: the caller vouches that a non-NULL `slot` is writable and aligned.
        unsafe { slot.write(value) };
    }
}

/// An output buffer and the place for the length written to it, checked before any work.
#[derive(Debug)]
pub(super) struct Output {
    buffer: *mut u8,
    capacity: usize,
    len: *mut usize,
}

impl Output {
    /// Checks the pointer rules: a NULL buffer with capacity 0 asks for the size, while a NULL
    /// buffer with room, or no place for the length, is a caller bug.
    pub(super) fn new(buffer: *mut u8, capacity: usize, len: *mut usize) -> Result<Output> {
        if buffer.is_null() && capacity != 0 {
            return Err(Error::InvalidArgument {
                reason: "an output buffer is NULL but its capacity is not 0",
            });
        }
        if len.is_null() {
            return Err(Error::InvalidArgument {
                reason: "an output length pointer is NULL",
            });
        }
        Ok(Output {
            buffer,
            capacity,
            len,
        })
    }

    /// Writes `bytes` and their length, or only the length when they do not fit, which is what a
    /// retry needs. Nothing is written past the bytes, not even a terminating NUL.
    ///
    /// # Errors
    /// [`Error::BufferTooSmall`] when the bytes do not fit.
    ///
    /// # Safety
    /// The pointers given to [`Output::new`] must still be valid: a non-NULL buffer with
    /// `capacity` writable bytes that do not overlap `bytes`, and a writable, aligned `size_t`
    /// for the length.
    pub(super) unsafe fn write(&self, bytes: &[u8]) -> Result<()> {
        // SAFETY: `new` checked that `len` is not NULL, and the caller vouches that it is
        // writable and aligned.
        unsafe { self.len.write(bytes.len()) };
        if bytes.len() > self.capacity {
            return Err(Error::BufferTooSmall {
                required: bytes.len(),
            });
        }
        if !bytes.is_empty() {
            // SAFETY: `bytes` is not empty and fits the capacity, which is therefore not 0, so
            // `new` saw a non-NULL buffer; the caller vouches for `capacity` writable bytes there
            // that do not overlap `bytes`.
            unsafe { ptr::copy_nonoverlapping(bytes.as_ptr(), self.buffer, bytes.len()) };
        }
        Ok(())
    }
}

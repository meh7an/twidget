//! Panic containment and the thread-local last error. Every exported function runs its body
//! through [`run`], so a panic never unwinds into the caller, and every failure leaves a
//! description behind for `twidget_last_error`.

use alloc::format;
use alloc::string::{String, ToString};
use core::any::Any;
use core::cell::Cell;
use core::mem;
use std::panic::{self, AssertUnwindSafe};

use crate::{Error, Result};

std::thread_local! {
    /// The calling thread's most recent failure, as `twidget_last_error` copies it.
    static LAST_ERROR: Cell<String> = const { Cell::new(String::new()) };
}

/// Runs the body of an exported function. A failure is recorded as the thread's last error before
/// it is returned, and a panic becomes [`Error::Internal`], recorded with the panic's message.
pub(super) fn run<T>(body: impl FnOnce() -> Result<T>) -> Result<T> {
    // Asserting unwind safety holds because bodies commit to a handle only once nothing can fail,
    // so a panic leaves every handle as it was.
    match panic::catch_unwind(AssertUnwindSafe(body)) {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(error)) => {
            record(error.to_string());
            Err(error)
        }
        Err(payload) => {
            record(contained(&*payload));
            // Dropping the payload runs its destructor, which could panic in turn and unwind into
            // the caller, so it is leaked instead.
            mem::forget(payload);
            Err(Error::Internal)
        }
    }
}

/// The last error for a contained panic, with the panic's message when it has one.
fn contained(payload: &(dyn Any + Send)) -> String {
    let message = payload
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| payload.downcast_ref::<String>().map(String::as_str));
    match message {
        Some(message) => format!("{}: {message}", Error::Internal),
        None => Error::Internal.to_string(),
    }
}

fn record(message: String) {
    // While the thread is being torn down the slot may be gone, and the message goes with it.
    let _ = LAST_ERROR.try_with(|last| last.set(message));
}

/// A copy of the calling thread's last error, empty when no call on it has failed.
pub(super) fn last_error() -> String {
    LAST_ERROR
        .try_with(|last| {
            let message = last.take();
            let copy = message.clone();
            last.set(message);
            copy
        })
        .unwrap_or_default()
}

#[cfg(test)]
std::thread_local! {
    static ARMED: Cell<Option<&'static str>> = const { Cell::new(None) };
    static PASSED: Cell<alloc::vec::Vec<&'static str>> = const { Cell::new(alloc::vec::Vec::new()) };
}

/// Marks where an exported function has done its work and is about to commit it. Tests count the
/// probes each call passes, and can arm one to panic there.
#[cfg(test)]
pub(super) fn probe(point: &'static str) {
    PASSED.with(|passed| {
        let mut all = passed.take();
        all.push(point);
        passed.set(all);
    });
    if ARMED.with(Cell::get) == Some(point) {
        panic!("{point} panicked where a test armed it");
    }
}

/// Marks where an exported function has done its work and is about to commit it; only tests
/// act on it.
#[cfg(not(test))]
pub(super) fn probe(_point: &'static str) {}

/// Arms `point` to panic on this thread, or disarms every probe with `None`.
#[cfg(test)]
pub(super) fn arm(point: Option<&'static str>) {
    ARMED.with(|armed| armed.set(point));
}

/// How many times this thread has passed `point`.
#[cfg(test)]
pub(super) fn passes(point: &str) -> usize {
    PASSED.with(|passed| {
        let all = passed.take();
        let count = all.iter().filter(|passed| **passed == point).count();
        passed.set(all);
        count
    })
}

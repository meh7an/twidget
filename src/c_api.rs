//! The C ABI that `include/twidget.h` declares: opaque handles, UTF-8 byte slices in, status codes
//! out, and caller-provided output buffers.
//!
//! Every exported function runs its whole body inside [`guard::run`], so a panic never unwinds
//! into the caller, and reports a failure through its status and the thread-local last error.
//! Bodies parse and evaluate into temporaries and commit to a handle only once nothing else can
//! fail, so a failed call, a contained panic included, leaves every handle as it was.

mod buffer;
mod guard;
#[cfg(test)]
mod header;

use alloc::boxed::Box;
use alloc::string::String;
use alloc::sync::Arc;
use core::ptr;

use crate::expr::Env;
use crate::model::{self, Template};
use crate::resolve::Instance;
use crate::{Error, Result, Status};
use buffer::Output;

/// `TWIDGET_ABI_VERSION`: the ABI this library implements.
const ABI_VERSION: u32 = 1;

/// `TWIDGET_NO_WAKEUP`: the wake-up written when a tree does not read the clock.
const NO_WAKEUP: i64 = -1;

/// `TWidgetTemplate`: a checked template. Instances built from it hold their own reference, so
/// the caller may drop it first.
#[derive(Debug)]
pub struct TWidgetTemplate {
    template: Arc<Template>,
}

// The header lets templates be shared across threads.
const _: () = {
    const fn shared<T: Send + Sync>() {}
    shared::<TWidgetTemplate>();
};

/// `TWidgetInstance`: a widget bound to its template, with the last tree and summary it wrote,
/// which a call with the same environment copies instead of evaluating again.
#[derive(Debug)]
pub struct TWidgetInstance {
    instance: Instance,
    resolved: Cached<(String, Option<i64>)>,
    summary: Cached<String>,
}

/// The last result a function wrote and the environment it was evaluated for; with no
/// environment it is empty, and its value is never read.
#[derive(Debug, Default)]
struct Cached<T> {
    env: Option<Env>,
    value: T,
}

/// `TWidgetEnv`, laid out as the header declares it.
#[repr(C)]
#[derive(Debug)]
pub struct TWidgetEnv {
    /// `sizeof(TWidgetEnv)` in the caller's header; a size below this header's is rejected.
    pub struct_size: u32,
    /// Server-synced Unix time in milliseconds.
    pub now_ms: i64,
    /// The viewer's current UTC offset in minutes.
    pub tz_offset_minutes: i32,
    /// Nonzero when the active theme is dark.
    pub dark: u32,
    /// Nonzero when the layout direction is right to left.
    pub rtl: u32,
}

/// The status a function returns for `result`; [`guard::run`] has already recorded a failure as
/// the last error.
fn code<T>(result: &Result<T>) -> i32 {
    match result {
        Ok(_) => Status::Ok.code(),
        Err(error) => error.status().code(),
    }
}

/// The instance behind a handle, which must not be NULL.
///
/// # Safety
/// `instance` must be NULL or a live handle from `twidget_instance_new` that nothing else is
/// using during the call.
unsafe fn handle<'a>(instance: *mut TWidgetInstance) -> Result<&'a mut TWidgetInstance> {
    // SAFETY: the caller vouches that a non-NULL handle is live and not in use elsewhere.
    unsafe { instance.as_mut() }.ok_or(Error::InvalidArgument {
        reason: "instance is NULL",
    })
}

/// The value `cache` holds for `env`, or else the one `evaluate` returns, which replaces it once
/// the probe `point` has passed.
fn cached<'a, T>(
    cache: &'a mut Cached<T>,
    env: Env,
    point: &'static str,
    evaluate: impl FnOnce() -> Result<T>,
) -> Result<&'a T> {
    if cache.env != Some(env) {
        let value = evaluate()?;
        guard::probe(point);
        *cache = Cached {
            env: Some(env),
            value,
        };
    }
    Ok(&cache.value)
}

/// `twidget_abi_version`: the ABI version of the linked library, which the caller compares with
/// `TWIDGET_ABI_VERSION` at startup. It reads no pointers, so it is safe to call.
#[no_mangle]
pub extern "C" fn twidget_abi_version() -> u32 {
    guard::run(|| {
        guard::probe("twidget_abi_version");
        Ok(ABI_VERSION)
    })
    // No header declares version 0, so a contained panic fails the caller's startup check.
    .unwrap_or(0)
}

/// `twidget_envelope_template_ref`: reads an envelope's `template_ref` without building anything,
/// so the caller can load the bundled template it names. On success it writes the id and its
/// length, and the version when `version_out` is not NULL.
///
/// # Safety
/// `envelope_ptr` must point to `envelope_len` readable bytes or be NULL with length 0, `id_out`
/// must point to `id_out_cap` writable bytes or be NULL with capacity 0, `id_len` must point to a
/// writable `size_t`, and `version_out` must be NULL or point to a writable `uint32_t`.
#[no_mangle]
pub unsafe extern "C" fn twidget_envelope_template_ref(
    envelope_ptr: *const u8,
    envelope_len: usize,
    id_out: *mut u8,
    id_out_cap: usize,
    id_len: *mut usize,
    version_out: *mut u32,
) -> i32 {
    code(&guard::run(|| {
        let id = Output::new(id_out, id_out_cap, id_len)?;
        // SAFETY: the caller vouches for the envelope's bytes.
        let envelope = unsafe { buffer::input(envelope_ptr, envelope_len) }?;
        let reference = model::read_template_ref(envelope)?;
        guard::probe("twidget_envelope_template_ref");
        // SAFETY: the caller vouches for `id_out_cap` writable bytes at `id_out` and for `id_len`.
        unsafe { id.write(reference.id.as_bytes()) }?;
        // SAFETY: the caller vouches that `version_out` is NULL or writable.
        unsafe { buffer::put(version_out, reference.version) };
        Ok(())
    }))
}

/// `twidget_template_new`: parses, checks and budget-checks a template, such as a bundled one.
/// It returns NULL on failure, and writes the status to `status_out` unless that is NULL.
///
/// # Safety
/// `json_ptr` must point to `json_len` readable bytes or be NULL with length 0, and `status_out`
/// must be NULL or point to a writable `int32_t`.
#[no_mangle]
pub unsafe extern "C" fn twidget_template_new(
    json_ptr: *const u8,
    json_len: usize,
    status_out: *mut i32,
) -> *mut TWidgetTemplate {
    let result = guard::run(|| {
        // SAFETY: the caller vouches for the template's bytes.
        let json = unsafe { buffer::input(json_ptr, json_len) }?;
        let template = Arc::new(Template::parse(json)?);
        guard::probe("twidget_template_new");
        Ok(Box::into_raw(Box::new(TWidgetTemplate { template })))
    });
    // SAFETY: the caller vouches that `status_out` is NULL or writable.
    unsafe { buffer::put(status_out, code(&result)) };
    result.unwrap_or(ptr::null_mut())
}

/// `twidget_template_drop`: releases the caller's reference to a template; instances built from
/// it keep their own. NULL is accepted.
///
/// # Safety
/// `tpl` must be NULL or a handle from `twidget_template_new` that has not been dropped.
#[no_mangle]
pub unsafe extern "C" fn twidget_template_drop(tpl: *mut TWidgetTemplate) {
    // A drop has no status to return; a failure stays in the last error.
    let _ = guard::run(|| {
        guard::probe("twidget_template_drop");
        if !tpl.is_null() {
            // SAFETY: the caller vouches that the handle came from `twidget_template_new` and is
            // dropped only once.
            drop(unsafe { Box::from_raw(tpl) });
        }
        Ok(())
    });
}

/// `twidget_instance_new`: binds an envelope to its template, the one it carries inline when
/// `bundled` is NULL, or else `bundled`, which must match its `template_ref`. It returns NULL on
/// failure, and writes the status to `status_out` unless that is NULL.
///
/// # Safety
/// `envelope_ptr` must point to `envelope_len` readable bytes or be NULL with length 0,
/// `bundled` must be NULL or a live handle from `twidget_template_new`, and `status_out` must be
/// NULL or point to a writable `int32_t`.
#[no_mangle]
pub unsafe extern "C" fn twidget_instance_new(
    envelope_ptr: *const u8,
    envelope_len: usize,
    bundled: *const TWidgetTemplate,
    status_out: *mut i32,
) -> *mut TWidgetInstance {
    let result = guard::run(|| {
        // SAFETY: the caller vouches for the envelope's bytes.
        let envelope = unsafe { buffer::input(envelope_ptr, envelope_len) }?;
        // SAFETY: the caller vouches that a non-NULL `bundled` is a live template handle.
        let bundled = unsafe { bundled.as_ref() }.map(|bundled| Arc::clone(&bundled.template));
        let instance = Instance::new(envelope, bundled)?;
        guard::probe("twidget_instance_new");
        Ok(Box::into_raw(Box::new(TWidgetInstance {
            instance,
            resolved: Cached::default(),
            summary: Cached::default(),
        })))
    });
    // SAFETY: the caller vouches that `status_out` is NULL or writable.
    unsafe { buffer::put(status_out, code(&result)) };
    result.unwrap_or(ptr::null_mut())
}

/// `twidget_instance_drop`: frees an instance. NULL is accepted.
///
/// # Safety
/// `instance` must be NULL or a handle from `twidget_instance_new` that has not been dropped and
/// that nothing else is using.
#[no_mangle]
pub unsafe extern "C" fn twidget_instance_drop(instance: *mut TWidgetInstance) {
    // A drop has no status to return; a failure stays in the last error.
    let _ = guard::run(|| {
        guard::probe("twidget_instance_drop");
        if !instance.is_null() {
            // SAFETY: the caller vouches that the handle came from `twidget_instance_new` and is
            // dropped only once.
            drop(unsafe { Box::from_raw(instance) });
        }
        Ok(())
    });
}

/// `twidget_instance_set_state`: replaces the whole state, as when the widget message is edited,
/// and forgets the results written for the old one. On failure the old state stays in place.
///
/// # Safety
/// `instance` must be a live handle that nothing else is using, and `state_ptr` must point to
/// `state_len` readable bytes or be NULL with length 0.
#[no_mangle]
pub unsafe extern "C" fn twidget_instance_set_state(
    instance: *mut TWidgetInstance,
    state_ptr: *const u8,
    state_len: usize,
    state_version: i64,
) -> i32 {
    code(&guard::run(|| {
        // SAFETY: the caller vouches for the handle.
        let handle = unsafe { handle(instance) }?;
        // SAFETY: the caller vouches for the state's bytes.
        let bytes = unsafe { buffer::input(state_ptr, state_len) }?;
        let state = model::parse_state(bytes)?;
        guard::probe("twidget_instance_set_state");
        handle.instance.replace_state(state, state_version);
        handle.resolved = Cached::default();
        handle.summary = Cached::default();
        Ok(())
    }))
}

/// `twidget_resolve`: writes the resolved tree as JSON in format `TWIDGET_RESOLVED_FORMAT`, and
/// the server time to resolve again at, or `TWIDGET_NO_WAKEUP`, to `next_wakeup_ms` unless that
/// is NULL. When the buffer is too small it writes only the size, and a call with the same
/// environment copies the same result without evaluating again.
///
/// # Safety
/// `instance` must be a live handle that nothing else is using, `env` must point to a
/// `TWidgetEnv` of at least its `struct_size`, `out` must point to `out_cap` writable bytes or be
/// NULL with capacity 0, `out_len` must point to a writable `size_t`, and `next_wakeup_ms` must
/// be NULL or point to a writable `int64_t`.
#[no_mangle]
pub unsafe extern "C" fn twidget_resolve(
    instance: *mut TWidgetInstance,
    env: *const TWidgetEnv,
    out: *mut u8,
    out_cap: usize,
    out_len: *mut usize,
    next_wakeup_ms: *mut i64,
) -> i32 {
    code(&guard::run(|| {
        // SAFETY: the caller vouches for the handle.
        let handle = unsafe { handle(instance) }?;
        // SAFETY: the caller vouches for the environment.
        let env = unsafe { buffer::env(env) }?;
        let output = Output::new(out, out_cap, out_len)?;
        let (json, wakeup) = cached(&mut handle.resolved, env, "twidget_resolve", || {
            let resolved = handle.instance.resolve(&env)?;
            Ok((resolved.json(), resolved.next_wakeup_ms))
        })?;
        // SAFETY: the caller vouches for `out_cap` writable bytes at `out` and for `out_len`.
        unsafe { output.write(json.as_bytes()) }?;
        // SAFETY: the caller vouches that `next_wakeup_ms` is NULL or writable.
        unsafe { buffer::put(next_wakeup_ms, wakeup.unwrap_or(NO_WAKEUP)) };
        Ok(())
    }))
}

/// `twidget_summary`: writes the template's summary as plain text for chat lists, reply previews
/// and notifications, empty when the template has none. The buffer works as in
/// `twidget_resolve`.
///
/// # Safety
/// `instance` must be a live handle that nothing else is using, `env` must point to a
/// `TWidgetEnv` of at least its `struct_size`, `out` must point to `out_cap` writable bytes or be
/// NULL with capacity 0, and `out_len` must point to a writable `size_t`.
#[no_mangle]
pub unsafe extern "C" fn twidget_summary(
    instance: *mut TWidgetInstance,
    env: *const TWidgetEnv,
    out: *mut u8,
    out_cap: usize,
    out_len: *mut usize,
) -> i32 {
    code(&guard::run(|| {
        // SAFETY: the caller vouches for the handle.
        let handle = unsafe { handle(instance) }?;
        // SAFETY: the caller vouches for the environment.
        let env = unsafe { buffer::env(env) }?;
        let output = Output::new(out, out_cap, out_len)?;
        let text = cached(&mut handle.summary, env, "twidget_summary", || {
            handle.instance.summary(&env)
        })?;
        // SAFETY: the caller vouches for `out_cap` writable bytes at `out` and for `out_len`.
        unsafe { output.write(text.as_bytes()) }
    }))
}

/// `twidget_last_error`: copies the calling thread's most recent failure into `out`, cut at
/// `out_cap` bytes, and returns its full length, which may exceed `out_cap`. It copies nothing
/// when `out` is NULL, and the text is empty until a call on the thread fails.
///
/// # Safety
/// `out` must be NULL or point to `out_cap` writable bytes.
#[no_mangle]
pub unsafe extern "C" fn twidget_last_error(out: *mut u8, out_cap: usize) -> usize {
    guard::run(|| {
        guard::probe("twidget_last_error");
        let message = guard::last_error();
        let copied = message.len().min(out_cap);
        if !out.is_null() && copied > 0 {
            // SAFETY: `out` is not NULL, and the caller vouches for `out_cap` writable bytes
            // there, of which `copied` is at most all.
            unsafe { ptr::copy_nonoverlapping(message.as_ptr(), out, copied) };
        }
        Ok(message.len())
    })
    .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    use alloc::vec::Vec;
    use core::ffi::c_void;
    use core::mem::{self, MaybeUninit};
    use std::path::PathBuf;
    use std::{fs, thread};

    use crate::json::{self, Limits, Value};

    // The functions as C code declares them, so every call below goes through the exported
    // symbol. Handles are opaque to C, so they are `void` pointers here.
    extern "C" {
        fn twidget_abi_version() -> u32;
        fn twidget_envelope_template_ref(
            envelope_ptr: *const u8,
            envelope_len: usize,
            id_out: *mut u8,
            id_out_cap: usize,
            id_len: *mut usize,
            version_out: *mut u32,
        ) -> i32;
        fn twidget_template_new(
            json_ptr: *const u8,
            json_len: usize,
            status_out: *mut i32,
        ) -> *mut c_void;
        fn twidget_template_drop(tpl: *mut c_void);
        fn twidget_instance_new(
            envelope_ptr: *const u8,
            envelope_len: usize,
            bundled: *const c_void,
            status_out: *mut i32,
        ) -> *mut c_void;
        fn twidget_instance_drop(instance: *mut c_void);
        fn twidget_instance_set_state(
            instance: *mut c_void,
            state_ptr: *const u8,
            state_len: usize,
            state_version: i64,
        ) -> i32;
        fn twidget_resolve(
            instance: *mut c_void,
            env: *const TWidgetEnv,
            out: *mut u8,
            out_cap: usize,
            out_len: *mut usize,
            next_wakeup_ms: *mut i64,
        ) -> i32;
        fn twidget_summary(
            instance: *mut c_void,
            env: *const TWidgetEnv,
            out: *mut u8,
            out_cap: usize,
            out_len: *mut usize,
        ) -> i32;
        fn twidget_last_error(out: *mut u8, out_cap: usize) -> usize;
    }

    const OK: i32 = Status::Ok.code();
    const INVALID_ARGUMENT: i32 = Status::InvalidArgument.code();
    const BUFFER_TOO_SMALL: i32 = Status::BufferTooSmall.code();
    const INTERNAL: i32 = Status::Internal.code();

    const FIXTURES: [&str; 8] = [
        "chart",
        "chess",
        "chess_start",
        "clock",
        "flow",
        "kanban",
        "shapes",
        "textbox",
    ];

    fn read(parts: &[&str]) -> Vec<u8> {
        let path: PathBuf = [env!("CARGO_MANIFEST_DIR")].iter().chain(parts).collect();
        fs::read(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
    }

    fn envelope(name: &str) -> Vec<u8> {
        read(&["fixtures", "envelopes", &format!("{name}.json")])
    }

    /// The golden tree of a fixture in the fixed environment, without the file's newline.
    fn golden(name: &str) -> Vec<u8> {
        let file = format!("{name}.light.ltr.json");
        let mut golden = read(&["fixtures", "expected", "resolved", &file]);
        assert_eq!(golden.pop(), Some(b'\n'));
        golden
    }

    /// An envelope whose inline template has `root` as its root.
    fn inline(root: &str, state: &str) -> Vec<u8> {
        format!(
            "{{\"tgw\": 1, \"template\": {{\"spec\": 1, \"id\": \"tg.test\", \"version\": 1, \
             \"root\": {root}}}, \"state\": {state}}}"
        )
        .into_bytes()
    }

    /// `env` as C code passes it.
    fn c_env(env: Env) -> TWidgetEnv {
        TWidgetEnv {
            struct_size: u32::try_from(mem::size_of::<TWidgetEnv>()).unwrap(),
            now_ms: env.now_ms,
            tz_offset_minutes: env.tz_offset_minutes,
            dark: u32::from(env.dark),
            rtl: u32::from(env.rtl),
        }
    }

    /// The fixed environment one second later, when the clock shows another second.
    fn a_second_later() -> TWidgetEnv {
        c_env(Env {
            now_ms: Env::TEST.now_ms + 1000,
            ..Env::TEST
        })
    }

    /// The fixed environment written into `env` with every padding byte set to `fill`, as C
    /// code may leave them.
    fn fill_padded(env: &mut MaybeUninit<TWidgetEnv>, fill: u8) {
        let fields = c_env(Env::TEST);
        let raw = env.as_mut_ptr();
        // SAFETY: `raw` points to a whole TWidgetEnv, and every write stays inside it.
        unsafe {
            ptr::write_bytes(raw.cast::<u8>(), fill, mem::size_of::<TWidgetEnv>());
            ptr::addr_of_mut!((*raw).struct_size).write(fields.struct_size);
            ptr::addr_of_mut!((*raw).now_ms).write(fields.now_ms);
            ptr::addr_of_mut!((*raw).tz_offset_minutes).write(fields.tz_offset_minutes);
            ptr::addr_of_mut!((*raw).dark).write(fields.dark);
            ptr::addr_of_mut!((*raw).rtl).write(fields.rtl);
        }
    }

    /// The calling thread's last error, read through the symbol.
    fn last_error() -> String {
        // SAFETY: a NULL buffer with capacity 0 is never written.
        let len = unsafe { twidget_last_error(ptr::null_mut(), 0) };
        let mut text = alloc::vec![0; len];
        // SAFETY: `text` has `len` writable bytes.
        assert_eq!(unsafe { twidget_last_error(text.as_mut_ptr(), len) }, len);
        String::from_utf8(text).unwrap()
    }

    /// A template built through the symbol, or NULL, with the status it reported.
    fn template(json: &[u8]) -> (*mut c_void, i32) {
        let mut status = 1;
        // SAFETY: `json` and `status` are live for the call.
        let template = unsafe { twidget_template_new(json.as_ptr(), json.len(), &mut status) };
        (template, status)
    }

    /// An instance built through the symbol, or NULL, with the status it reported.
    fn instance(envelope: &[u8], bundled: *const c_void) -> (*mut c_void, i32) {
        let mut status = 1;
        // SAFETY: `envelope` and `status` are live for the call, and `bundled` is NULL or a live
        // template.
        let instance = unsafe {
            twidget_instance_new(envelope.as_ptr(), envelope.len(), bundled, &mut status)
        };
        (instance, status)
    }

    /// The template an envelope names, built from the fixtures, or NULL when it carries its own.
    fn bundled_for(envelope: &[u8]) -> *mut c_void {
        let mut id = [0; 64];
        let (mut id_len, mut version) = (0, 0);
        // SAFETY: every pointer is to a live local of the size passed with it.
        let status = unsafe {
            twidget_envelope_template_ref(
                envelope.as_ptr(),
                envelope.len(),
                id.as_mut_ptr(),
                id.len(),
                &mut id_len,
                &mut version,
            )
        };
        if status == Status::NotFound.code() {
            return ptr::null_mut();
        }
        assert_eq!(status, OK, "{}", last_error());
        let id = core::str::from_utf8(&id[..id_len]).unwrap();
        let (template, status) = template(&read(&[
            "fixtures",
            "templates",
            &format!("{id}@{version}.json"),
        ]));
        assert_eq!(status, OK, "{}", last_error());
        template
    }

    /// An instance of the fixture `name`, with the template it names, as an app builds it.
    fn fixture(name: &str) -> (*mut c_void, *mut c_void) {
        let envelope = envelope(name);
        let bundled = bundled_for(&envelope);
        let (instance, status) = instance(&envelope, bundled);
        assert_eq!(status, OK, "{name}: {}", last_error());
        (instance, bundled)
    }

    /// The tree and wake-up an instance resolves to, asked for as apps do: the size first, then
    /// the bytes into a buffer that fits.
    fn resolved(instance: *mut c_void, env: *const TWidgetEnv) -> (Vec<u8>, i64) {
        let mut len = 0;
        // SAFETY: the instance and environment are live, and a NULL buffer with capacity 0 is
        // never written.
        let status = unsafe {
            twidget_resolve(instance, env, ptr::null_mut(), 0, &mut len, ptr::null_mut())
        };
        assert_eq!(status, BUFFER_TOO_SMALL, "{}", last_error());
        let mut out = alloc::vec![0; len];
        let (mut written, mut wakeup) = (0, 0);
        // SAFETY: `out` has `len` writable bytes, and the rest are live locals.
        let status = unsafe {
            twidget_resolve(
                instance,
                env,
                out.as_mut_ptr(),
                len,
                &mut written,
                &mut wakeup,
            )
        };
        assert_eq!(status, OK, "{}", last_error());
        assert_eq!(written, len);
        (out, wakeup)
    }

    /// The summary of an instance, asked for the same way.
    fn summary(instance: *mut c_void, env: *const TWidgetEnv) -> String {
        let mut len = 0;
        // SAFETY: the instance and environment are live, and a NULL buffer with capacity 0 is
        // never written.
        let status = unsafe { twidget_summary(instance, env, ptr::null_mut(), 0, &mut len) };
        if len == 0 {
            assert_eq!(status, OK);
            return String::new();
        }
        assert_eq!(status, BUFFER_TOO_SMALL, "{}", last_error());
        let mut out = alloc::vec![0; len];
        let mut written = 0;
        // SAFETY: `out` has `len` writable bytes.
        let status = unsafe { twidget_summary(instance, env, out.as_mut_ptr(), len, &mut written) };
        assert_eq!(status, OK, "{}", last_error());
        assert_eq!(written, len);
        String::from_utf8(out).unwrap()
    }

    /// The number of trees and summaries this thread has evaluated through the API.
    fn evaluations() -> (usize, usize) {
        (
            guard::passes("twidget_resolve"),
            guard::passes("twidget_summary"),
        )
    }

    /// Frees an instance and the template it was built with, which may be NULL.
    fn release((instance, bundled): (*mut c_void, *mut c_void)) {
        // SAFETY: both handles are live, dropped here once, and NULL is accepted.
        unsafe {
            twidget_instance_drop(instance);
            twidget_template_drop(bundled);
        }
    }

    /// The Rust API's instance of an envelope, to compare the symbols' output with.
    fn rust_instance(envelope: &[u8]) -> Instance {
        let bundled = model::read_template_ref(envelope).ok().map(|reference| {
            let file = format!("{}@{}.json", reference.id, reference.version);
            Arc::new(Template::parse(&read(&["fixtures", "templates", &file])).unwrap())
        });
        Instance::new(envelope, bundled).unwrap()
    }

    #[test]
    fn test_fixtures_through_the_symbols_match_the_goldens() {
        // SAFETY: the function reads no pointers.
        assert_eq!(unsafe { twidget_abi_version() }, ABI_VERSION);
        let env = c_env(Env::TEST);
        for name in FIXTURES {
            let handles = fixture(name);
            let golden = golden(name);
            assert_eq!(resolved(handles.0, &env).0, golden, "{name}");
            let rust = rust_instance(&envelope(name));
            assert_eq!(summary(handles.0, &env), rust.summary(&Env::TEST).unwrap());
            // Setting the state the envelope already carries, empty when it has none, writes
            // the same tree again.
            let Ok(Value::Object(members)) = json::parse(&envelope(name), &Limits::ENVELOPE) else {
                panic!("{name} is not an object");
            };
            let state = members
                .get("state")
                .map_or_else(|| String::from("{}"), json::to_string);
            // SAFETY: the instance is live, and `state` outlives the call.
            let status =
                unsafe { twidget_instance_set_state(handles.0, state.as_ptr(), state.len(), 2) };
            assert_eq!(status, OK, "{name}: {}", last_error());
            assert_eq!(resolved(handles.0, &env).0, golden, "{name}");
            release(handles);
        }
    }

    #[test]
    fn test_failures_return_their_status_and_describe_it() {
        assert_eq!(template(b"{"), (ptr::null_mut(), Status::Json.code()));
        assert!(last_error().starts_with("malformed JSON at byte 1: "));
        let dotless = br#"{"spec": 1, "id": "x", "version": 1, "root": {"type": "text"}}"#;
        assert_eq!(template(dotless), (ptr::null_mut(), Status::Schema.code()));
        assert!(last_error().starts_with("schema error at "));
        let long = inline(
            r#"{"type": "text"}"#,
            &format!("{{\"note\": \"{}\"}}", "x".repeat(5000)),
        );
        assert_eq!(
            instance(&long, ptr::null()),
            (ptr::null_mut(), Status::Budget.code())
        );
        assert_eq!(
            last_error(),
            "budget exceeded: inline envelope (limit 4096)"
        );
        let newer = br#"{"tgw": 2, "template": {"spec": 1, "id": "tg.test", "version": 1,
            "root": {"type": "text"}}, "state": {}}"#;
        assert_eq!(
            instance(newer, ptr::null()),
            (ptr::null_mut(), Status::UnsupportedVersion.code())
        );
        let chess = envelope("chess");
        assert_eq!(
            instance(&chess, ptr::null()),
            (ptr::null_mut(), Status::TemplateRequired.code())
        );
        let kanban = bundled_for(&envelope("kanban"));
        assert_eq!(
            instance(&chess, kanban),
            (ptr::null_mut(), Status::TemplateMismatch.code())
        );
        let plain = inline(r#"{"type": "text"}"#, "{}");
        assert_eq!(
            instance(&plain, kanban),
            (ptr::null_mut(), Status::TemplateMismatch.code())
        );
        let clock = envelope("clock");
        let mut id_len = 0;
        // SAFETY: the envelope and `id_len` are live, and a NULL id buffer with capacity 0 is
        // never written.
        let status = unsafe {
            twidget_envelope_template_ref(
                clock.as_ptr(),
                clock.len(),
                ptr::null_mut(),
                0,
                &mut id_len,
                ptr::null_mut(),
            )
        };
        assert_eq!(status, Status::NotFound.code());
        assert_eq!(
            last_error(),
            "the envelope carries an inline template, so it has no reference"
        );
        // A budget passed while resolving fails the call and writes nothing.
        let repeat = r#"{"type": "column", "children": [{"type": "repeat",
            "each": {"=": "range(1001)"}, "child": {"type": "divider"}}]}"#;
        let (costly, status) = instance(&inline(repeat, "{}"), ptr::null());
        assert_eq!(status, OK);
        let mut len = 7;
        // SAFETY: the instance and the environment are live, and a NULL buffer with capacity 0
        // is never written.
        let status = unsafe {
            twidget_resolve(
                costly,
                &c_env(Env::TEST),
                ptr::null_mut(),
                0,
                &mut len,
                ptr::null_mut(),
            )
        };
        assert_eq!((status, len), (Status::Budget.code(), 7));
        release((costly, kanban));
    }

    #[test]
    fn test_null_input_is_empty_only_with_length_0() {
        let (clock, _) = fixture("clock");
        let mut id_len = 0;
        let mut status = 1;
        // SAFETY: each NULL input comes with the length under test, every other pointer is to a
        // live local, and a NULL buffer with capacity 0 is never written.
        unsafe {
            // An empty input is read, and then fails as malformed JSON.
            let empty = twidget_envelope_template_ref(
                ptr::null(),
                0,
                ptr::null_mut(),
                0,
                &mut id_len,
                ptr::null_mut(),
            );
            assert_eq!(empty, Status::Json.code());
            let missing = twidget_envelope_template_ref(
                ptr::null(),
                1,
                ptr::null_mut(),
                0,
                &mut id_len,
                ptr::null_mut(),
            );
            assert_eq!(missing, INVALID_ARGUMENT);
            assert_eq!(
                last_error(),
                "invalid argument: an input pointer is NULL but its length is not 0"
            );
            assert!(twidget_template_new(ptr::null(), 0, &mut status).is_null());
            assert_eq!(status, Status::Json.code());
            assert!(twidget_template_new(ptr::null(), 1, &mut status).is_null());
            assert_eq!(status, INVALID_ARGUMENT);
            assert!(twidget_instance_new(ptr::null(), 0, ptr::null(), &mut status).is_null());
            assert_eq!(status, Status::Json.code());
            assert!(twidget_instance_new(ptr::null(), 1, ptr::null(), &mut status).is_null());
            assert_eq!(status, INVALID_ARGUMENT);
            let empty = twidget_instance_set_state(clock, ptr::null(), 0, 2);
            assert_eq!(empty, Status::Json.code());
            let missing = twidget_instance_set_state(clock, ptr::null(), 1, 2);
            assert_eq!(missing, INVALID_ARGUMENT);
            // `status_out` may be NULL.
            assert!(twidget_template_new(ptr::null(), 0, ptr::null_mut()).is_null());
        }
        release((clock, ptr::null_mut()));
    }

    #[test]
    fn test_output_buffers_answer_size_queries_and_change_only_on_success() {
        let env = c_env(Env::TEST);
        let (clock, _) = fixture("clock");
        let expected = golden("clock");
        let (mut len, mut wakeup) = (0, 7);
        let mut out = alloc::vec![0xEE; expected.len() + 8];
        // SAFETY: every buffer comes with its true capacity or with 0, and every other pointer is
        // NULL or a live local.
        unsafe {
            // A NULL buffer with capacity 0 asks for the size and gets nothing else.
            let query = twidget_resolve(clock, &env, ptr::null_mut(), 0, &mut len, &mut wakeup);
            assert_eq!((query, len, wakeup), (BUFFER_TOO_SMALL, expected.len(), 7));
            // One byte short, the buffer stays as it was.
            let short = expected.len() - 1;
            let status =
                twidget_resolve(clock, &env, out.as_mut_ptr(), short, &mut len, &mut wakeup);
            assert_eq!((status, wakeup), (BUFFER_TOO_SMALL, 7));
            assert!(out.iter().all(|byte| *byte == 0xEE));
            // With room, the tree arrives with nothing written after it, not even a NUL.
            let status = twidget_resolve(
                clock,
                &env,
                out.as_mut_ptr(),
                out.len(),
                &mut len,
                &mut wakeup,
            );
            assert_eq!(status, OK);
            assert_eq!(&out[..len], expected);
            assert!(out[len..].iter().all(|byte| *byte == 0xEE));
            // A NULL buffer with room, or no place for the length, is a caller bug.
            let null_with_room =
                twidget_resolve(clock, &env, ptr::null_mut(), 1, &mut len, ptr::null_mut());
            assert_eq!(null_with_room, INVALID_ARGUMENT);
            let no_len = twidget_resolve(
                clock,
                &env,
                out.as_mut_ptr(),
                out.len(),
                ptr::null_mut(),
                ptr::null_mut(),
            );
            assert_eq!(no_len, INVALID_ARGUMENT);
            let null_with_room = twidget_summary(clock, &env, ptr::null_mut(), 1, &mut len);
            assert_eq!(null_with_room, INVALID_ARGUMENT);
            let no_len = twidget_summary(clock, &env, out.as_mut_ptr(), out.len(), ptr::null_mut());
            assert_eq!(no_len, INVALID_ARGUMENT);
        }
        release((clock, ptr::null_mut()));
        // An empty summary answers a size query with success and length 0.
        let (plain, _) = instance(&inline(r#"{"type": "text"}"#, "{}"), ptr::null());
        let mut len = 9;
        // SAFETY: the instance, the environment and `len` are live, and a NULL buffer with
        // capacity 0 is never written.
        let status = unsafe { twidget_summary(plain, &env, ptr::null_mut(), 0, &mut len) };
        assert_eq!((status, len), (OK, 0));
        release((plain, ptr::null_mut()));
    }

    #[test]
    fn test_template_ref_id_follows_the_buffer_rules() {
        let chess = envelope("chess");
        let mut id = [0xEE; 16];
        let (mut id_len, mut version) = (0, 0);
        let call = |id_out: *mut u8, id_out_cap: usize, id_len: *mut usize, version: *mut u32| {
            // SAFETY: `chess` is live, and each test below passes NULL or live locals with their
            // true sizes.
            unsafe {
                twidget_envelope_template_ref(
                    chess.as_ptr(),
                    chess.len(),
                    id_out,
                    id_out_cap,
                    id_len,
                    version,
                )
            }
        };
        assert_eq!(
            call(ptr::null_mut(), 0, &mut id_len, &mut version),
            BUFFER_TOO_SMALL
        );
        assert_eq!((id_len, version), (8, 0));
        assert_eq!(
            call(id.as_mut_ptr(), 7, &mut id_len, &mut version),
            BUFFER_TOO_SMALL
        );
        assert!(id.iter().all(|byte| *byte == 0xEE));
        assert_eq!(
            call(ptr::null_mut(), 8, &mut id_len, &mut version),
            INVALID_ARGUMENT
        );
        assert_eq!(
            call(id.as_mut_ptr(), 16, ptr::null_mut(), &mut version),
            INVALID_ARGUMENT
        );
        assert_eq!(call(id.as_mut_ptr(), 16, &mut id_len, ptr::null_mut()), OK);
        assert_eq!((&id[..id_len], version), (&b"tg.chess"[..], 0));
        assert_eq!(call(id.as_mut_ptr(), 8, &mut id_len, &mut version), OK);
        assert_eq!(version, 1);
    }

    #[test]
    fn test_env_below_this_headers_size_is_rejected_and_a_larger_one_read() {
        let (plain, _) = instance(&inline(r#"{"type": "text"}"#, "{}"), ptr::null());
        let mut small = c_env(Env::TEST);
        small.struct_size -= 1;
        /// A newer header's `TWidgetEnv`, with a field appended.
        #[repr(C)]
        struct Newer {
            env: TWidgetEnv,
            _appended: u64,
        }
        let newer = Newer {
            env: TWidgetEnv {
                struct_size: u32::try_from(mem::size_of::<Newer>()).unwrap(),
                ..c_env(Env::TEST)
            },
            _appended: u64::MAX,
        };
        let mut len = 0;
        // SAFETY: the instance and every environment are live, and a NULL buffer with capacity
        // 0 is never written.
        unsafe {
            let small_env =
                twidget_resolve(plain, &small, ptr::null_mut(), 0, &mut len, ptr::null_mut());
            assert_eq!(small_env, INVALID_ARGUMENT);
            let no_env = twidget_resolve(
                plain,
                ptr::null(),
                ptr::null_mut(),
                0,
                &mut len,
                ptr::null_mut(),
            );
            assert_eq!(no_env, INVALID_ARGUMENT);
            let small_env = twidget_summary(plain, &small, ptr::null_mut(), 0, &mut len);
            assert_eq!(small_env, INVALID_ARGUMENT);
        }
        let newer = ptr::addr_of!(newer).cast::<TWidgetEnv>();
        assert_eq!(
            resolved(plain, newer).0,
            resolved(plain, &c_env(Env::TEST)).0
        );
        release((plain, ptr::null_mut()));
    }

    #[test]
    fn test_null_handles_are_rejected_and_dropping_null_does_nothing() {
        let env = c_env(Env::TEST);
        let mut len = 0;
        // SAFETY: NULL handles are the case under test, and the rest are live locals.
        unsafe {
            twidget_template_drop(ptr::null_mut());
            twidget_instance_drop(ptr::null_mut());
            let state = twidget_instance_set_state(ptr::null_mut(), b"{}".as_ptr(), 2, 2);
            assert_eq!(state, INVALID_ARGUMENT);
            let tree = twidget_resolve(
                ptr::null_mut(),
                &env,
                ptr::null_mut(),
                0,
                &mut len,
                ptr::null_mut(),
            );
            assert_eq!(tree, INVALID_ARGUMENT);
            let text = twidget_summary(ptr::null_mut(), &env, ptr::null_mut(), 0, &mut len);
            assert_eq!(text, INVALID_ARGUMENT);
        }
        assert_eq!(last_error(), "invalid argument: instance is NULL");
    }

    #[test]
    fn test_retry_with_the_same_env_copies_without_evaluating_again() {
        let handles = fixture("clock");
        let env = c_env(Env::TEST);
        let before = evaluations();
        // Each helper asks for the size, which evaluates, and then copies what it cached.
        let (tree, _) = resolved(handles.0, &env);
        let text = summary(handles.0, &env);
        assert_eq!(evaluations(), (before.0 + 1, before.1 + 1));
        assert_eq!(resolved(handles.0, &env).0, tree);
        assert_eq!(summary(handles.0, &env), text);
        assert_eq!(evaluations(), (before.0 + 1, before.1 + 1));
        // A second later is another environment, and the clock shows it.
        assert_ne!(resolved(handles.0, &a_second_later()).0, tree);
        assert_eq!(evaluations(), (before.0 + 2, before.1 + 1));
        release(handles);
    }

    #[test]
    fn test_retry_with_an_env_differing_only_in_padding_hits_the_cache() {
        let handles = fixture("clock");
        let (mut zeros, mut ones) = (MaybeUninit::uninit(), MaybeUninit::uninit());
        fill_padded(&mut zeros, 0x00);
        fill_padded(&mut ones, 0xFF);
        let before = evaluations();
        let (tree, _) = resolved(handles.0, zeros.as_ptr());
        assert_eq!(resolved(handles.0, ones.as_ptr()).0, tree);
        assert_eq!(evaluations(), (before.0 + 1, before.1));
        release(handles);
    }

    #[test]
    fn test_wakeup_follows_the_clock_or_says_there_is_none() {
        let env = c_env(Env::TEST);
        let clock = fixture("clock");
        let chart = fixture("chart");
        // The fixed time is 09:41:27.123, so the clock wakes at the next second.
        assert_eq!(resolved(clock.0, &env).1, Env::TEST.now_ms - 123 + 1000);
        assert_eq!(resolved(chart.0, &env).1, NO_WAKEUP);
        release(clock);
        release(chart);
    }

    #[test]
    fn test_set_state_keeps_the_old_state_on_failure_and_clears_the_caches_on_success() {
        let handles = fixture("clock");
        let env = c_env(Env::TEST);
        let (tree, _) = resolved(handles.0, &env);
        assert_eq!(summary(handles.0, &env), "Local time 13:11");
        let before = evaluations();
        let huge = format!("{{\"label\": \"{}\"}}", "x".repeat(70_000));
        // SAFETY: the instance is live, and each state outlives its call.
        unsafe {
            let schema = twidget_instance_set_state(handles.0, b"[]".as_ptr(), 2, 2);
            assert_eq!(schema, Status::Schema.code());
            let json = twidget_instance_set_state(handles.0, b"{".as_ptr(), 1, 2);
            assert_eq!(json, Status::Json.code());
            let budget = twidget_instance_set_state(handles.0, huge.as_ptr(), huge.len(), 2);
            assert_eq!(budget, Status::Budget.code());
        }
        // Nothing changed, so the cached results still stand.
        assert_eq!(resolved(handles.0, &env).0, tree);
        assert_eq!(summary(handles.0, &env), "Local time 13:11");
        assert_eq!(evaluations(), before);
        let renamed = br#"{"label": "Tehran"}"#;
        // SAFETY: the instance is live, and `renamed` outlives the call.
        let status =
            unsafe { twidget_instance_set_state(handles.0, renamed.as_ptr(), renamed.len(), 3) };
        assert_eq!(status, OK);
        assert_ne!(resolved(handles.0, &env).0, tree);
        assert_eq!(summary(handles.0, &env), "Tehran 13:11");
        assert_eq!(evaluations(), (before.0 + 1, before.1 + 1));
        release(handles);
    }

    #[test]
    fn test_last_error_truncates_to_the_buffer_and_stays_with_its_thread() {
        assert_eq!(thread::spawn(last_error).join().unwrap(), "");
        assert_eq!(template(b"{").1, Status::Json.code());
        let message = last_error();
        let mut short = [0xEE; 8];
        // SAFETY: `short` has 8 writable bytes, and a NULL buffer is never written.
        unsafe {
            assert_eq!(twidget_last_error(short.as_mut_ptr(), 8), message.len());
            assert_eq!(twidget_last_error(ptr::null_mut(), 8), message.len());
        }
        assert_eq!(&short[..], &message.as_bytes()[..8]);
        // A call that succeeds leaves the last failure in place.
        let (plain, status) = instance(&inline(r#"{"type": "text"}"#, "{}"), ptr::null());
        assert_eq!(status, OK);
        assert_eq!(last_error(), message);
        // Another thread's failure stays on that thread.
        let other = thread::spawn(|| {
            assert_eq!(template(b"{}").1, Status::Schema.code());
            last_error()
        });
        assert_ne!(other.join().unwrap(), message);
        assert_eq!(last_error(), message);
        release((plain, ptr::null_mut()));
    }

    #[test]
    fn test_template_dropped_before_its_instance_still_serves_it() {
        let (instance, bundled) = fixture("chess");
        // SAFETY: the template is live and dropped once; the instance holds its own reference.
        unsafe { twidget_template_drop(bundled) };
        assert_eq!(resolved(instance, &c_env(Env::TEST)).0, golden("chess"));
        release((instance, ptr::null_mut()));
    }

    /// Runs `call` with the probe `point` armed, and checks that the panic there was contained
    /// and described.
    fn contained<T>(point: &'static str, call: impl FnOnce() -> T) -> T {
        guard::arm(Some(point));
        let result = call();
        guard::arm(None);
        let expected = format!(
            "a defect inside the library was contained: {point} panicked where a test armed it"
        );
        assert_eq!(last_error(), expected);
        result
    }

    #[test]
    fn test_panics_in_every_call_are_contained_and_change_nothing() {
        let env = c_env(Env::TEST);
        let later = a_second_later();
        let chess = envelope("chess");
        let clock = envelope("clock");
        let json = read(&["fixtures", "templates", "tg.chess@1.json"]);
        let (mut id_len, mut version, mut status, mut len) = (7, 7, 7, 7);
        let mut id = [0; 16];
        // SAFETY: handles are live until their one successful drop, buffers come with their
        // true capacities or with 0, and every other pointer is NULL or a live local.
        unsafe {
            assert_eq!(
                contained("twidget_abi_version", || twidget_abi_version()),
                0
            );
            let reference = contained("twidget_envelope_template_ref", || {
                twidget_envelope_template_ref(
                    chess.as_ptr(),
                    chess.len(),
                    id.as_mut_ptr(),
                    id.len(),
                    &mut id_len,
                    &mut version,
                )
            });
            assert_eq!((reference, id_len, version), (INTERNAL, 7, 7));
            let template = contained("twidget_template_new", || {
                twidget_template_new(json.as_ptr(), json.len(), &mut status)
            });
            assert_eq!((template, status), (ptr::null_mut(), INTERNAL));
            // A drop that panics frees nothing, so the handle can still be dropped.
            let (template, _) = self::template(&json);
            contained("twidget_template_drop", || twidget_template_drop(template));
            twidget_template_drop(template);
            let instance = contained("twidget_instance_new", || {
                twidget_instance_new(clock.as_ptr(), clock.len(), ptr::null(), &mut status)
            });
            assert_eq!((instance, status), (ptr::null_mut(), INTERNAL));
            let (instance, _) = self::instance(&clock, ptr::null());
            let (tree, _) = resolved(instance, &env);
            let text = summary(instance, &env);
            let before = evaluations();
            // A panic replacing the state keeps the old one, and the cached results with it.
            let renamed = br#"{"label": "Tehran"}"#;
            let replaced = contained("twidget_instance_set_state", || {
                twidget_instance_set_state(instance, renamed.as_ptr(), renamed.len(), 3)
            });
            assert_eq!(replaced, INTERNAL);
            // A panic evaluating for a new environment keeps the results for the old one.
            let tree_later = contained("twidget_resolve", || {
                twidget_resolve(
                    instance,
                    &later,
                    ptr::null_mut(),
                    0,
                    &mut len,
                    ptr::null_mut(),
                )
            });
            let text_later = contained("twidget_summary", || {
                twidget_summary(instance, &later, ptr::null_mut(), 0, &mut len)
            });
            assert_eq!((tree_later, text_later, len), (INTERNAL, INTERNAL, 7));
            assert_eq!(resolved(instance, &env).0, tree);
            assert_eq!(summary(instance, &env), text);
            assert_eq!(evaluations(), (before.0 + 1, before.1 + 1));
            // Evaluating the new environment now shows the old state.
            assert_eq!(summary(instance, &later), text);
            contained("twidget_instance_drop", || twidget_instance_drop(instance));
            twidget_instance_drop(instance);
            // A panic reading the last error returns 0 and leaves its own description.
            assert_eq!(
                contained("twidget_last_error", || twidget_last_error(
                    ptr::null_mut(),
                    0
                )),
                0
            );
        }
    }
}

//! The header drift test: each function `include/twidget.h` declares is exported here with exactly
//! its signature, and the header's `#define` values and `TWidgetEnv` layout match the Rust side.
//! The tests in `src/error.rs` compare the status enum.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::mem;

use super::{TWidgetEnv, TWidgetInstance, TWidgetTemplate, ABI_VERSION, NO_WAKEUP};
use crate::resolve;

const HEADER: &str = include_str!("../../include/twidget.h");

/// A function's return type, name, and parameters as (type, name), every type spelled as C
/// spells it.
type Function = (String, String, Vec<(String, String)>);

/// Records an exported function's Rust signature as text, and assigns the function to a pointer
/// of exactly that type, which stops compiling once the text drifts from the code.
macro_rules! signature {
    ($name:ident($($parameter:ident: $type:ty),*) -> $returns:ty) => {{
        let _: unsafe extern "C" fn($($type),*) -> $returns = super::$name;
        let parameters: Vec<(String, String)> =
            Vec::from([$((c_type(stringify!($type)), String::from(stringify!($parameter)))),*]);
        (c_type(stringify!($returns)), String::from(stringify!($name)), parameters)
    }};
}

/// Records `TWidgetEnv`'s fields as text with their offsets: the struct literal stops compiling
/// unless the list names every field with its exact type.
macro_rules! fields {
    ($($field:ident: $type:ty),*) => {{
        let _ = TWidgetEnv { $($field: <$type>::default()),* };
        Vec::from([$((
            c_type(stringify!($type)),
            String::from(stringify!($field)),
            mem::offset_of!(TWidgetEnv, $field),
        )),*])
    }};
}

/// Every exported function, in the header's order.
fn exported() -> Vec<Function> {
    Vec::from([
        signature!(twidget_abi_version() -> u32),
        signature!(twidget_envelope_template_ref(
            envelope_ptr: *const u8,
            envelope_len: usize,
            id_out: *mut u8,
            id_out_cap: usize,
            id_len: *mut usize,
            version_out: *mut u32
        ) -> i32),
        signature!(twidget_template_new(
            json_ptr: *const u8,
            json_len: usize,
            status_out: *mut i32
        ) -> *mut TWidgetTemplate),
        signature!(twidget_template_drop(tpl: *mut TWidgetTemplate) -> ()),
        signature!(twidget_instance_new(
            envelope_ptr: *const u8,
            envelope_len: usize,
            bundled: *const TWidgetTemplate,
            status_out: *mut i32
        ) -> *mut TWidgetInstance),
        signature!(twidget_instance_drop(instance: *mut TWidgetInstance) -> ()),
        signature!(twidget_instance_set_state(
            instance: *mut TWidgetInstance,
            state_ptr: *const u8,
            state_len: usize,
            state_version: i64
        ) -> i32),
        signature!(twidget_resolve(
            instance: *mut TWidgetInstance,
            env: *const TWidgetEnv,
            out: *mut u8,
            out_cap: usize,
            out_len: *mut usize,
            next_wakeup_ms: *mut i64
        ) -> i32),
        signature!(twidget_summary(
            instance: *mut TWidgetInstance,
            env: *const TWidgetEnv,
            out: *mut u8,
            out_cap: usize,
            out_len: *mut usize
        ) -> i32),
        signature!(twidget_last_error(out: *mut u8, out_cap: usize) -> usize),
    ])
}

/// The C spelling of a type from a Rust signature.
fn c_type(rust: &str) -> String {
    if let Some(pointee) = rust.strip_prefix("*const ") {
        return format!("const {} *", c_type(pointee));
    }
    if let Some(pointee) = rust.strip_prefix("*mut ") {
        return format!("{} *", c_type(pointee));
    }
    let c = match rust {
        "()" => "void",
        "u8" => "uint8_t",
        "u32" => "uint32_t",
        "i32" => "int32_t",
        "i64" => "int64_t",
        "usize" => "size_t",
        named => named,
    };
    String::from(c)
}

/// The header without comments or preprocessor lines, with its whitespace collapsed.
fn code() -> String {
    let mut code = String::new();
    for (index, piece) in HEADER.split("/*").enumerate() {
        let kept = if index == 0 {
            piece
        } else {
            piece.split_once("*/").map_or("", |(_, after)| after)
        };
        code.push_str(kept);
    }
    let lines: Vec<&str> = code
        .lines()
        .filter(|line| !line.trim_start().starts_with('#'))
        .collect();
    lines
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Splits a declaration into its type and its trailing name, as in `const uint8_t *json_ptr`.
fn split_name(declaration: &str) -> (String, String) {
    let declaration = declaration.trim();
    let name = declaration
        .rsplit(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .next()
        .unwrap();
    let type_ = declaration.strip_suffix(name).unwrap().trim();
    (type_.to_string(), name.to_string())
}

/// Every function the header declares, in order.
fn declared() -> Vec<Function> {
    code()
        .split(';')
        .filter_map(|statement| {
            let (head, parameters) = statement.split_once('(')?;
            let parameters = parameters.trim().strip_suffix(')')?.trim();
            let (returns, name) = split_name(head);
            let parameters = if parameters == "void" {
                Vec::new()
            } else {
                parameters.split(',').map(split_name).collect()
            };
            Some((returns, name, parameters))
        })
        .collect()
}

#[test]
fn test_header_functions_match_the_exported_signatures() {
    let declared = declared();
    assert_eq!(declared.len(), 10);
    assert_eq!(declared, exported());
}

#[test]
fn test_header_defines_match_the_constants() {
    let declared: Vec<(&str, &str)> = HEADER
        .lines()
        .filter_map(|line| line.strip_prefix("#define ")?.split_once(' '))
        .map(|(name, value)| (name, value.trim()))
        .collect();
    let constants = [
        ("TWIDGET_ABI_VERSION", format!("{ABI_VERSION}u")),
        ("TWIDGET_RESOLVED_FORMAT", format!("{}u", resolve::FORMAT)),
        ("TWIDGET_NO_WAKEUP", format!("({NO_WAKEUP})")),
    ];
    let constants: Vec<(&str, &str)> = constants
        .iter()
        .map(|(name, value)| (*name, value.as_str()))
        .collect();
    assert_eq!(declared, constants);
}

#[test]
fn test_header_env_matches_the_rust_layout() {
    let rust = fields!(
        struct_size: u32,
        now_ms: i64,
        tz_offset_minutes: i32,
        dark: u32,
        rtl: u32
    );
    let code = code();
    let (_, body) = code.split_once("typedef struct TWidgetEnv {").unwrap();
    let (body, _) = body.split_once("} TWidgetEnv;").unwrap();
    // C places each field at the next multiple of its size and pads the struct to a multiple of
    // its widest field.
    let (mut offset, mut widest) = (0, 1);
    let mut declared = Vec::new();
    for field in body.split(';').filter(|field| !field.trim().is_empty()) {
        let (type_, name) = split_name(field);
        let size = match type_.as_str() {
            "uint32_t" | "int32_t" => 4,
            "int64_t" => 8,
            other => panic!("no size known for {other}"),
        };
        offset = usize::next_multiple_of(offset, size);
        declared.push((type_, name, offset));
        offset += size;
        widest = widest.max(size);
    }
    assert_eq!(declared, rust);
    assert_eq!(
        mem::size_of::<TWidgetEnv>(),
        offset.next_multiple_of(widest)
    );
}

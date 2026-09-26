//! The crate-wide error type and the status codes of `include/twidget.h`.

use alloc::string::String;
use core::fmt;

/// A status code from `include/twidget.h`. These discriminants are the crate's only copy of the
/// header's numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
#[repr(i32)]
pub enum Status {
    /// `TWIDGET_OK`: the call succeeded.
    Ok = 0,
    /// `TWIDGET_ERROR_INVALID_ARGUMENT`: the caller passed an argument the API cannot accept.
    InvalidArgument = -1,
    /// `TWIDGET_ERROR_BUFFER_TOO_SMALL`: the output buffer cannot hold the result.
    BufferTooSmall = -2,
    /// `TWIDGET_ERROR_JSON`: malformed JSON.
    Json = -3,
    /// `TWIDGET_ERROR_SCHEMA`: well-formed JSON that breaks the spec.
    Schema = -4,
    /// `TWIDGET_ERROR_BUDGET`: a budget was exceeded while parsing or resolving.
    Budget = -5,
    /// `TWIDGET_ERROR_UNSUPPORTED_VERSION`: the document targets a newer format.
    UnsupportedVersion = -6,
    /// `TWIDGET_ERROR_TEMPLATE_REQUIRED`: the envelope needs a bundled template.
    TemplateRequired = -7,
    /// `TWIDGET_ERROR_TEMPLATE_MISMATCH`: the passed template does not fit the envelope.
    TemplateMismatch = -8,
    /// `TWIDGET_ERROR_NOT_FOUND`: the envelope has no template reference to read.
    NotFound = -9,
    /// `TWIDGET_ERROR_INTERNAL`: a defect inside the library was contained.
    Internal = -10,
}

impl Status {
    /// The value a C API function returns for this status.
    pub const fn code(self) -> i32 {
        self as i32
    }

    /// The name `include/twidget.h` gives this status.
    pub const fn name(self) -> &'static str {
        match self {
            Status::Ok => "TWIDGET_OK",
            Status::InvalidArgument => "TWIDGET_ERROR_INVALID_ARGUMENT",
            Status::BufferTooSmall => "TWIDGET_ERROR_BUFFER_TOO_SMALL",
            Status::Json => "TWIDGET_ERROR_JSON",
            Status::Schema => "TWIDGET_ERROR_SCHEMA",
            Status::Budget => "TWIDGET_ERROR_BUDGET",
            Status::UnsupportedVersion => "TWIDGET_ERROR_UNSUPPORTED_VERSION",
            Status::TemplateRequired => "TWIDGET_ERROR_TEMPLATE_REQUIRED",
            Status::TemplateMismatch => "TWIDGET_ERROR_TEMPLATE_MISMATCH",
            Status::NotFound => "TWIDGET_ERROR_NOT_FOUND",
            Status::Internal => "TWIDGET_ERROR_INTERNAL",
        }
    }
}

/// Everything the crate can fail with. Input of any shape, however hostile, surfaces as one of
/// these instead of a panic.
#[derive(Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Error {
    /// The input is not strict JSON.
    Json {
        /// Byte offset into the input where the problem was found.
        offset: usize,
        /// What is wrong at that offset.
        reason: &'static str,
    },
    /// The JSON is well formed but breaks the spec.
    Schema {
        /// Where the problem sits, as a path such as `root.children[2]`.
        location: String,
        /// The rule the document breaks.
        reason: &'static str,
    },
    /// A budget from the spec was exceeded while parsing or resolving.
    Budget {
        /// The budget's name, as the spec's budget table words it.
        budget: &'static str,
        /// The limit that was exceeded.
        limit: usize,
    },
    /// The document targets a newer format than this library supports.
    UnsupportedVersion {
        /// The version field that is too new, `tgw` or `spec`.
        field: &'static str,
    },
    /// The envelope references a bundled template, but none was passed.
    TemplateRequired,
    /// The passed template's id or version differs from the envelope's reference, or the
    /// envelope carries its own template.
    TemplateMismatch,
    /// The envelope carries an inline template, so it has no reference to read.
    NotFound,
    /// The caller passed an argument the API cannot accept.
    InvalidArgument {
        /// Which argument was rejected, and why.
        reason: &'static str,
    },
    /// The caller's output buffer is smaller than the result.
    BufferTooSmall {
        /// The number of bytes the result needs.
        required: usize,
    },
    /// A defect inside the library was contained, and the call had no effect.
    Internal,
}

impl Error {
    /// The status the C API reports for this error.
    pub const fn status(&self) -> Status {
        match self {
            Error::Json { .. } => Status::Json,
            Error::Schema { .. } => Status::Schema,
            Error::Budget { .. } => Status::Budget,
            Error::UnsupportedVersion { .. } => Status::UnsupportedVersion,
            Error::TemplateRequired => Status::TemplateRequired,
            Error::TemplateMismatch => Status::TemplateMismatch,
            Error::NotFound => Status::NotFound,
            Error::InvalidArgument { .. } => Status::InvalidArgument,
            Error::BufferTooSmall { .. } => Status::BufferTooSmall,
            Error::Internal => Status::Internal,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Json { offset, reason } => {
                write!(f, "malformed JSON at byte {offset}: {reason}")
            }
            Error::Schema { location, reason } => {
                write!(f, "schema error at {location}: {reason}")
            }
            Error::Budget { budget, limit } => {
                write!(f, "budget exceeded: {budget} (limit {limit})")
            }
            Error::UnsupportedVersion { field } => {
                write!(
                    f,
                    "`{field}` targets a newer version than this library supports"
                )
            }
            Error::TemplateRequired => {
                f.write_str("the envelope references a bundled template, but none was passed")
            }
            Error::TemplateMismatch => {
                f.write_str("the passed template does not match the envelope's reference")
            }
            Error::NotFound => {
                f.write_str("the envelope carries an inline template, so it has no reference")
            }
            Error::InvalidArgument { reason } => write!(f, "invalid argument: {reason}"),
            Error::BufferTooSmall { required } => {
                write!(f, "output buffer too small: {required} bytes required")
            }
            Error::Internal => f.write_str("a defect inside the library was contained"),
        }
    }
}

impl core::error::Error for Error {}

/// The crate's result type, `twidget::Result<T>`.
pub type Result<T> = core::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;

    const HEADER: &str = include_str!("../include/twidget.h");

    const STATUSES: [Status; 11] = [
        Status::Ok,
        Status::InvalidArgument,
        Status::BufferTooSmall,
        Status::Json,
        Status::Schema,
        Status::Budget,
        Status::UnsupportedVersion,
        Status::TemplateRequired,
        Status::TemplateMismatch,
        Status::NotFound,
        Status::Internal,
    ];

    fn one_error_per_variant() -> [Error; 10] {
        [
            Error::Json {
                offset: 17,
                reason: "unexpected byte",
            },
            Error::Schema {
                location: String::from("root.children[2]"),
                reason: "unknown node type",
            },
            Error::Budget {
                budget: "JSON nesting depth",
                limit: 64,
            },
            Error::UnsupportedVersion { field: "spec" },
            Error::TemplateRequired,
            Error::TemplateMismatch,
            Error::NotFound,
            Error::InvalidArgument {
                reason: "out_len is NULL",
            },
            Error::BufferTooSmall { required: 4096 },
            Error::Internal,
        ]
    }

    #[test]
    fn test_status_each_value_matches_the_header() {
        for status in STATUSES {
            let declaration = format!("{} = {},", status.name(), status.code());
            assert!(
                HEADER.contains(&declaration),
                "header lacks `{declaration}`"
            );
        }
    }

    #[test]
    fn test_status_header_declares_no_others() {
        let declared = HEADER
            .lines()
            .map(str::trim)
            .filter(|line| line.starts_with("TWIDGET_") && line.contains(" = "))
            .count();
        assert_eq!(declared, STATUSES.len());
    }

    #[test]
    fn test_error_each_variant_maps_to_its_own_failure_status() {
        let errors = one_error_per_variant();
        for status in STATUSES {
            let expected = usize::from(status != Status::Ok);
            let mapped = errors
                .iter()
                .filter(|error| error.status() == status)
                .count();
            assert_eq!(
                mapped,
                expected,
                "{} comes from {mapped} variants",
                status.name()
            );
        }
    }

    #[test]
    fn test_error_display_names_offset_location_and_budget() {
        let [json, schema, budget, ..] = one_error_per_variant();
        assert!(json.to_string().contains("byte 17"));
        assert!(schema.to_string().contains("root.children[2]"));
        let budget = budget.to_string();
        assert!(budget.contains("JSON nesting depth") && budget.contains("64"));
        assert!(Error::BufferTooSmall { required: 4096 }
            .to_string()
            .contains("4096"));
    }

    #[test]
    fn test_error_display_every_variant_has_a_message() {
        for error in one_error_per_variant() {
            assert!(!error.to_string().is_empty(), "{error:?} has no message");
        }
    }
}

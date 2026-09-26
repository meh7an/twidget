#ifndef TWIDGET_H
#define TWIDGET_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/*
 * twidget interprets Telegram live widget documents. It parses envelopes and
 * templates, enforces budgets while parsing, evaluates expressions, and writes
 * a resolved tree that each client lays out and paints natively.
 *
 * The library owns no threads, clocks, callbacks or I/O: time and theme
 * arrive through TWidgetEnv on every resolve. Every string is UTF-8, passed
 * as pointer plus length, and borrowed only for the duration of the call.
 * A NULL input pointer with length 0 is an empty input, and a NULL output
 * buffer with capacity 0 asks for the required size.
 *
 * Templates are immutable and may be shared across threads. An instance must
 * not be used from two threads at once.
 */

#define TWIDGET_ABI_VERSION 1u
#define TWIDGET_RESOLVED_FORMAT 1u
#define TWIDGET_NO_WAKEUP (-1)

typedef struct TWidgetTemplate TWidgetTemplate;
typedef struct TWidgetInstance TWidgetInstance;

enum TwidgetStatus {
  TWIDGET_OK = 0,
  TWIDGET_ERROR_INVALID_ARGUMENT = -1,
  TWIDGET_ERROR_BUFFER_TOO_SMALL = -2,
  /* Malformed JSON. */
  TWIDGET_ERROR_JSON = -3,
  /* Well-formed JSON that breaks the spec: unknown node type, wrong field type. */
  TWIDGET_ERROR_SCHEMA = -4,
  /* A budget was exceeded while parsing or resolving. */
  TWIDGET_ERROR_BUDGET = -5,
  /* The envelope or template targets a newer format than this library. */
  TWIDGET_ERROR_UNSUPPORTED_VERSION = -6,
  /* The envelope references a bundled template, but none was passed. */
  TWIDGET_ERROR_TEMPLATE_REQUIRED = -7,
  /* The passed template's id or version differs from the envelope's reference. */
  TWIDGET_ERROR_TEMPLATE_MISMATCH = -8,
  /* The envelope carries an inline template, so it has no reference to read. */
  TWIDGET_ERROR_NOT_FOUND = -9,
  /* A defect inside the library was contained; the call had no effect. */
  TWIDGET_ERROR_INTERNAL = -10,
};

/*
 * Inputs a resolve may read besides the widget itself. Set struct_size to
 * sizeof(TWidgetEnv); smaller values are rejected, and later versions can
 * append fields without breaking older callers.
 */
typedef struct TWidgetEnv {
  uint32_t struct_size;
  /* Server-synced Unix time, so every device agrees on clocks and countdowns. */
  int64_t now_ms;
  /* The viewer's current UTC offset; the library carries no time zone database. */
  int32_t tz_offset_minutes;
  /* Nonzero when the active theme is dark. */
  uint32_t dark;
  /* Nonzero when the layout direction is right to left. */
  uint32_t rtl;
} TWidgetEnv;

/* The ABI version of the linked library; compare with TWIDGET_ABI_VERSION at startup. */
uint32_t twidget_abi_version(void);

/*
 * Reads an envelope's template_ref without building anything, so the caller
 * can load the matching bundled template first. On success, writes the id into
 * id_out and sets *id_len and *version_out. Returns TWIDGET_ERROR_NOT_FOUND
 * for inline templates, and TWIDGET_ERROR_BUFFER_TOO_SMALL with *id_len set
 * when id_out is too small. version_out may be NULL.
 */
int32_t twidget_envelope_template_ref(
    const uint8_t *envelope_ptr,
    size_t envelope_len,
    uint8_t *id_out,
    size_t id_out_cap,
    size_t *id_len,
    uint32_t *version_out);

/*
 * Parses, validates and budget-checks a template. Returns NULL on failure.
 * status_out may be NULL; otherwise it receives TWIDGET_OK or the failure.
 */
TWidgetTemplate *twidget_template_new(
    const uint8_t *json_ptr,
    size_t json_len,
    int32_t *status_out);

/* NULL is accepted. Instances built from the template keep their own reference. */
void twidget_template_drop(TWidgetTemplate *tpl);

/*
 * Binds an envelope to its template. Pass NULL as bundled for an inline
 * template, or the template matching the envelope's template_ref; passing a
 * template with an inline envelope fails with TWIDGET_ERROR_TEMPLATE_MISMATCH.
 * Returns NULL on failure; status_out may be NULL.
 */
TWidgetInstance *twidget_instance_new(
    const uint8_t *envelope_ptr,
    size_t envelope_len,
    const TWidgetTemplate *bundled,
    int32_t *status_out);

/* NULL is accepted. Other pointers must have come from twidget_instance_new. */
void twidget_instance_drop(TWidgetInstance *instance);

/*
 * Replaces the whole state, for example after the widget message was edited.
 * On failure the previous state stays in place. state_version is
 * informational in v0 and orders updates once sync arrives.
 */
int32_t twidget_instance_set_state(
    TWidgetInstance *instance,
    const uint8_t *state_ptr,
    size_t state_len,
    int64_t state_version);

/*
 * Writes the resolved tree as UTF-8 JSON in format TWIDGET_RESOLVED_FORMAT.
 * On TWIDGET_ERROR_BUFFER_TOO_SMALL, *out_len holds the required size, and a
 * retry with the same env reuses the cached result instead of evaluating
 * again. Identical output bytes mean the tree did not change, so the caller
 * can skip layout.
 *
 * next_wakeup_ms may be NULL; otherwise it receives the server time at which
 * to resolve again, or TWIDGET_NO_WAKEUP when the tree does not read the clock.
 */
int32_t twidget_resolve(
    TWidgetInstance *instance,
    const TWidgetEnv *env,
    uint8_t *out,
    size_t out_cap,
    size_t *out_len,
    int64_t *next_wakeup_ms);

/*
 * Writes the template's summary as plain UTF-8 text for the chat list, reply
 * previews and notifications. Writes an empty string when the template has
 * none. Buffer handling matches twidget_resolve.
 */
int32_t twidget_summary(
    TWidgetInstance *instance,
    const TWidgetEnv *env,
    uint8_t *out,
    size_t out_cap,
    size_t *out_len);

/*
 * Copies a description of the calling thread's most recent failure into out
 * and returns its full length, which may exceed out_cap. Meant for logs, not
 * for display to users.
 */
size_t twidget_last_error(uint8_t *out, size_t out_cap);

#ifdef __cplusplus
}
#endif

#endif /* TWIDGET_H */
# twidget

twidget is the shared Rust interpreter for Telegram live widgets. It parses a widget message's
envelope and template under hard budgets, evaluates the template's expressions against its state,
and writes the resolved tree that Telegram Desktop and Telegram Android lay out and paint
natively, along with when to resolve again and a summary for chat lists.

The contracts live in this repository:

- [`docs/SPEC.md`](docs/SPEC.md), the behavior, revision v0.8.
- [`include/twidget.h`](include/twidget.h), the C ABI, version 1.
- [`docs/RESOLVED.md`](docs/RESOLVED.md), the resolved tree's JSON, format 1.

## Building

The toolchain is pinned to Rust 1.96.1 in `rust-toolchain.toml`, and the crate has no
dependencies.

```sh
cargo build
cargo test
```

`scripts/check.sh` is what every change must pass: formatting, then clippy with warnings denied
and the tests, for each of the 16 combinations of the crate's features.

```sh
scripts/check.sh
```

| Feature | What it adds                                                                         |
| ------- | ------------------------------------------------------------------------------------ |
| `std`   | On by default; the crate always links std, and `c-api` and `cli` need it              |
| `c-api` | The C API of `include/twidget.h`, the one module with unsafe code                     |
| `cli`   | The `twidget-cli` binary, the reference layout and the SVG preview                    |
| `wasm`  | The reference layout and the SVG preview, kept for a WebAssembly build                |

The static library an app links:

```sh
cargo rustc --lib --release --locked --features c-api --crate-type staticlib
```

It lands at `target/release/libtwidget.a` and exports exactly the ten functions of the header.

## The command line

`twidget-cli` resolves a widget, lays it out with the conformance tests' fake text metrics, or
draws it as SVG, all at a fixed test time unless `--now` and `--tz` say otherwise:

```sh
cargo run --features cli -- resolve fixtures/envelopes/clock.json
cargo run --features cli -- layout fixtures/envelopes/kanban.json --width 320 --rtl
cargo run --features cli -- svg fixtures/envelopes/chess_start.json --dark > /tmp/chess_start.svg
```

`cargo run --features cli -- --help` lists every option. Envelopes that name a bundled template
find it in `fixtures/templates`, or wherever `--templates` points.

## Tests and goldens

The fixtures in `fixtures/envelopes` are locked as golden files: resolved trees in
`fixtures/expected/resolved`, and the reference layout's frames in `fixtures/expected/frames`.
Only these commands rewrite them, and a rewrite is reviewed as a diff before it is committed:

```sh
TWIDGET_BLESS=1 cargo test --test goldens
TWIDGET_BLESS=1 cargo test --features cli --test layout
```

A seeded mutation suite runs thousands of mangled fixtures through the Rust API on every test run,
and through the C API when `c-api` is on. Worst-case documents at every budget are timed by an
ignored test, which only means something in a release build:

```sh
cargo test --release --test worst_case -- --ignored --nocapture
```

## For app integrators

A widget message is a text message whose first `pre` entity with the language `tgw` holds the
widget's envelope. Everything below goes through `include/twidget.h`, and any failure means one
thing: show the message as ordinary text. A broken widget never breaks a chat.

**At startup**, check `twidget_abi_version() == TWIDGET_ABI_VERSION`, and leave widgets off when
it differs.

**Templates.** An envelope carries its template inline or names a bundled one. Ask with
`twidget_envelope_template_ref`: `TWIDGET_ERROR_NOT_FOUND` means the template is inline, so pass
NULL as `bundled`. Otherwise it writes the id and the version, and the app loads
`tgw/templates/<id>@<version>.json` from its resources, builds it once with
`twidget_template_new`, and keeps it. A template is immutable, reference-counted and safe to share
across threads.

**Instances.** `twidget_instance_new` binds an envelope to its template. The instance holds its
own reference, so the app may drop the template first. An instance is not thread-safe: use each
one from one thread at a time, and free it with `twidget_instance_drop`, which accepts NULL.

**Resolving.** Fill a `TWidgetEnv` with `struct_size = sizeof(TWidgetEnv)`, the server-synced time,
the viewer's UTC offset, and whether the theme is dark and the layout right to left. Every output
buffer works the same way: pass NULL with capacity 0 to learn the size, which comes back with
`TWIDGET_ERROR_BUFFER_TOO_SMALL`, then call again with a buffer that fits and the same env, and
the cached result is copied without evaluating again. Output is UTF-8 without a terminating NUL,
written only on success. A resolved tree is at most 1 MB, and identical bytes mean nothing changed,
so layout can be skipped.

```c
TWidgetEnv env = {sizeof(TWidgetEnv), server_now_ms, tz_offset_minutes, dark, rtl};
size_t len = 0;
int64_t wakeup = TWIDGET_NO_WAKEUP;
int32_t status = twidget_resolve(instance, &env, NULL, 0, &len, NULL);
if (status == TWIDGET_ERROR_BUFFER_TOO_SMALL) {
    uint8_t *json = malloc(len);
    status = twidget_resolve(instance, &env, json, len, &len, &wakeup);
    if (status == TWIDGET_OK) {
        /* Lay out and paint json[0..len), and arm a timer for wakeup while visible. */
    }
    free(json);
}
if (status != TWIDGET_OK) {
    /* Show the message as ordinary text. */
}
```

**Wake-ups.** A tree that reads the clock returns the server time to resolve again at, and
`TWIDGET_NO_WAKEUP` otherwise. Arm a timer for it only while the widget is visible.

**Edits.** Editing a widget message replaces its state. Pass the edited envelope's `state` to
`twidget_instance_set_state`; a failure leaves the previous state in place.

**Summaries.** Use `twidget_summary`, with the same buffer protocol, for the chat list, reply
previews and notifications; it is empty when the template has none.

**Logs.** `twidget_last_error` copies the calling thread's most recent failure, for logs and not
for users. A defect inside the crate is contained and reported as `TWIDGET_ERROR_INTERNAL`, and
the call has no effect.

**Painting.** The resolved format is documented in [`docs/RESOLVED.md`](docs/RESOLVED.md). Colors
arrive as theme tokens, so a theme change is a repaint with no crate call, and keys stay stable
across resolves, so moved nodes can animate. To test a native layout, inject the fake text
metrics of the spec's conformance section and compare against `fixtures/expected/frames`.

## License

None yet: the crate is `publish = false` until the team decides.

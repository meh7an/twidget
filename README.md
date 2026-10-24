# twidget

twidget is the shared Rust interpreter for Telegram live widgets. It parses a widget message's
envelope and template under hard budgets, evaluates the template's expressions against its state,
and writes the resolved tree that Telegram Desktop and Telegram Android lay out and paint
natively, along with when to resolve again and a summary for chat lists.

The contracts live in this repository:

- [`docs/SPEC.md`](docs/SPEC.md), the behavior, revision v0.16.
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

A static library for this machine:

```sh
cargo rustc --lib --release --locked --features c-api --crate-type staticlib
```

It lands at `target/release/libtwidget.a` and exports exactly the ten functions of the header.

## Release libraries

The libraries the apps ship are built on a Mac into `prebuild/`, which git ignores. Telegram
Desktop links `prebuild/lib/macos/libtwidget.a`, one universal library for arm64 and x86_64 built
for macOS 10.13 and later. Telegram Android links `prebuild/lib/<ABI>/libtwidget.a` for
`armeabi-v7a`, `arm64-v8a`, `x86` and `x86_64`, built with NDK 27.2.12479018 for API level 21.
Windows and Linux libraries come from Telegram Desktop's `prepare.py` instead.

```sh
cargo install cargo-ndk@4.1.2 --locked
scripts/build-macos.sh
scripts/build-android.sh
scripts/check-prebuild.sh
```

A script missing a Rust target names the `rustup target add` command that installs it. The Android
script and the check take the NDK from the Android SDK at `$ANDROID_HOME`, or at
`~/Library/Android/sdk` when that is unset. `prebuild/BUILD_INFO` has a line for each library
with the commit it was built from, marked `-dirty` when the tree had uncommitted changes, the rustc
version, and the macOS version or the NDK version and API level it targets, and for Android the
number of global symbols it defines, `exports=10`.

Each Android library is one object whose only global symbols are the header's ten functions.
Telegram Android links it into one shared library beside tlottie and its wallet engine, Rust
libraries with their own `rust_eh_personality` and std symbols, so the build script prelinks every
member of cargo's archive with the NDK's `ld.lld -r`, makes every other definition local with
`llvm-objcopy`, and removes the section groups through which the app's linker could swap twidget's
personality routine for another library's. `scripts/check-prebuild.sh` confirms that each Android
library is that one object, then links `tests/c/resolve.c` for each ABI beside a stand-in
`rust_eh_personality` and checks that twidget's unwind entries still name its own routine.

`scripts/check-prebuild.sh` confirms that the macOS library holds both architectures and that every
library defines each function of the header. It then builds `tests/c/resolve.c` with the header
against the macOS library, resolves every fixture through it on each architecture the Mac can run,
and compares the trees with the goldens. Every setting, from the crate and its features to the NDK
version, sits at the top of `scripts/prebuild.sh`, which each script sources.

## The command line

`twidget-cli` resolves a widget, lays it out with the conformance tests' fake text metrics, draws
it as SVG, or samples its animations, all at a fixed test time unless `--now` and `--tz` say
otherwise:

```sh
cargo run --features cli -- resolve fixtures/envelopes/clock.json
cargo run --features cli -- layout fixtures/envelopes/kanban.json --width 320 --rtl
cargo run --features cli -- svg fixtures/envelopes/chess_start.json --dark > /tmp/chess_start.svg
cargo run --features cli -- sample fixtures/envelopes/motion.json --at 1790415687823
```

`sample` writes each animation's value at the server time `--at`, for a widget that appeared at
`--appear`, which defaults to `--now`. SVGs draw the resting frame.

`cargo run --features cli -- --help` lists every option. Envelopes that name a bundled template
find it in `fixtures/templates`, or wherever `--templates` points.

## Tests and goldens

The fixtures in `fixtures/envelopes` are locked as golden files: resolved trees in
`fixtures/expected/resolved`, the reference layout's frames in `fixtures/expected/frames`, and the
animated fixture's sampled values in `fixtures/expected/sampled`. Only these commands rewrite them,
and a rewrite is reviewed as a diff before it is committed:

```sh
TWIDGET_BLESS=1 cargo test --test goldens
TWIDGET_BLESS=1 cargo test --features cli --test layout
TWIDGET_BLESS=1 cargo test --features cli --test sampled
```

## Animating a widget

A node animates paint-only properties with keyframes in `animate`, and a template eases between
resolves with `transition`. Looping entries run on server time, so every viewer sees the same
frame; `once` and counted ones start when the widget first appears. Values may be expressions, as
the delay that staggers these bars is:

```json
{"type": "rect", "x": {"=": "index * 24 + 4"}, "y": 8, "w": 16, "h": 24, "fill": "accent",
 "animate": [{"property": "h", "duration": 900, "delay": {"=": "index * 150"}, "repeat": "loop",
   "keyframes": [{"at": 0, "value": 8, "easing": "ease_in_out"}, {"at": 0.5, "value": 32,
     "easing": "ease_in_out"}, {"at": 1, "value": 8}]}]}
```

A keyframe without a `value` takes the node's own, so an intro ends where the node rests. The
template's `transition`, such as `{"duration": 300}`, is every node's default, and a node replaces
it with its own, such as `{"text": "roll_up"}` on a clock, or turns it off with `"none"`. The
spec's *Animation* clarification lists what each node type animates, and
`fixtures/envelopes/motion.json` uses most of it.

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
the viewer's UTC offset, whether the theme is dark, and whether the widget lays out right to left.
Resolve first with `rtl` 0. When the first strong character of the text the tree draws is right to
left, or the tree draws none and the app is right to left, resolve again with `rtl` 1, and keep
that direction for the envelope's later resolves. Every output buffer works the same way: pass
NULL with capacity 0 to learn the size, which comes back with `TWIDGET_ERROR_BUFFER_TOO_SMALL`,
then call again with a buffer that fits and the same env, and the cached result is copied without
evaluating again. Output is UTF-8 without a terminating NUL, written only on success. A resolved
tree is at most 1 MB, and identical bytes mean nothing changed, so layout can be skipped.

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
previews and notifications; it is empty when the template has none. When it is empty or only
whitespace, show the message's text outside the `tgw` block, trimmed, and when that is empty too, a
fixed label such as "Widget". Compute it when the message loads and again on an edit.

**Logs.** `twidget_last_error` copies the calling thread's most recent failure, for logs and not
for users. A defect inside the crate is contained and reported as `TWIDGET_ERROR_INTERNAL`, and
the call has no effect.

**Painting.** The resolved format is documented in [`docs/RESOLVED.md`](docs/RESOLVED.md). Colors
arrive as theme tokens, so a theme change is a repaint with no crate call, and keys stay stable
across resolves, so moved nodes can animate. Every container clips its content to its box, except
that a `scroll_x`'s slid content may draw sideways into free space up to the bubble's edges, as the
spec's *Scroll strip bleed* defines. To test a native layout, inject the fake text metrics of the
spec's conformance section and compare against `fixtures/expected/frames`.

**Animation.** A tree carries a `motion` member beside `root`, `null` when nothing moves: the
animation entries and the transitions nodes take, keyed by node, with every value worked out.
Interpolate at draw time while the widget is visible, with no crate call per frame, running
`loop` and `alternate` entries on server time and the rest from when the widget first appears.
When a new tree replaces the old one after a state change, an edit or a clock tick, match nodes by
key and ease between them as the spec's *Animation* clarification describes; a theme, direction or
width change replaces the tree at once. With the system's animation scale at 0 or power saving
on, show the resting tree and skip transitions. To test an interpolator, compare it against
`fixtures/expected/sampled`, which `twidget-cli sample` writes.

## License

None yet: the crate is `publish = false` until the team decides.

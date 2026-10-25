# Live widgets spec v1

As of 2026-10-25, revision v1.0. This repository copy leads the original design doc and is the
source of truth for implementation. v0.1 adds the Implementation clarifications section at the end,
v0.2 settles the defaults, required properties and paint values that section left open, v0.3
settles the expression edge cases it left open, v0.4 and v0.5 settle the resolver's, v0.6 the
reference layout's, v0.7 the SVG fonts, v0.8 bounds the resolved tree's size, v0.9 settles the
release artifacts, v0.10 makes each Android library export only the C API, v0.11 says whose
direction `env.rtl` follows, v0.12 lets slid `scroll_x` content draw past the strip, v0.13 takes
`env.rtl` from the widget's own text instead, v0.14 says what previews show without a summary,
v0.15 adds keyframe and transition animation, v0.16 settles three of its details, v0.17 lets a
`use`'s placement transition and orders repeated entries, and v1.0 adds interactivity: actions,
triggers, state styles and sync through a bot.

## Overview

v0 proves native live widgets end to end on TDesktop and Android with seven sample widgets,
delivered as ordinary text messages because the backend does not exist yet. Live widgets are
declarative documents that every client renders natively inside a message bubble, matching the
chat's theme and layout.

v0 is render-first. Widgets display their state, follow the theme and layout direction, and tick
locally (the clock). Shared interactivity lands in v1, under *Interactivity (v1.0)*.

Out of scope for v0:

- Backend support, TL schema and Bot API methods
- Actions, drag and drop, text editing and permission enforcement
- Forwarding semantics, secret chats and a full-screen viewer
- TDLib and third-party clients, which can link the same library later

Guiding rule, taken from how the clients already work: unify what must be exact across clients,
and keep per client what must feel native. Lottie is the precedent: one shared interpreter
(tlottie), with bubble integration written per client.

## Architecture

One Rust crate, `twidget`, interprets widget documents identically on every client, and each
client lays out and paints the result natively. The split mirrors the apps today: tlottie, tde2e
and tgcalls are shared engines, while text, math and markdown layout are written per client.

| Concern                                                      | Owner           | Why                                                             |
| ------------------------------------------------------------ | --------------- | --------------------------------------------------------------- |
| Envelope and template parsing, validation, budgets           | `twidget` crate | Untrusted input that must behave identically everywhere         |
| Expressions, `repeat` expansion, symbols, connector geometry | `twidget` crate | The same document must resolve to the same tree on every client |
| Time dependency and next wake-up                             | `twidget` crate | Clock ticks must agree                                          |
| Layout                                                       | Each client     | Depends on native text metrics                                  |
| Painting and theme token resolution                          | Each client     | Native look, separate incoming and outgoing bubble palettes     |
| Measuring and drawing text                                   | Each client     | Existing text engines handle entities and custom emoji          |
| Detection, template lookup, timers, caching, threading       | Each client     | Platform integration                                            |

Data flow: the client detects a `tgw` code block, the crate builds an instance, the crate
resolves it with the current environment, and the client lays out and paints the resolved tree.
When the tree reads the clock, the crate also returns the next wake-up time, and the client
resolves again then while the widget is visible.

Source seam: clients read widgets through one interface with four verbs: resolve template, get
state, subscribe to changes, and send action. v0 implements it over plain-text messages and leaves
send action empty; v1 sends actions through the message's buttons and replies, to a bot that
edits the message. The MTProto implementation replaces it in v2 without touching the crate or
the renderers.

The crate copies tlottie's house style: zero default dependencies, clippy denying panicking
constructs, opaque handles, pointer-plus-length inputs, status codes, and caller-provided output
buffers. It exposes the features `c-api`, `wasm` and `cli`.

## Transport v0: plain-text widget messages

A widget message is a normal text message containing a code block whose language is `tgw`, and
that block holds the widget envelope as JSON. No server changes are needed, and unpatched clients
simply show the text.

Message layout:

- Text outside the code block is fallback for unpatched clients, for example "Chess: Mehran vs
  Ali".
- The first `pre` entity whose language is exactly `tgw` is the envelope; later `tgw` blocks are
  ignored.
- Patched clients render the whole message as the widget and hide both the fallback text and the
  code block.

Detection and failure: a client treats a message as a widget only when the crate accepts its
envelope. Any error renders the message as ordinary text, so a broken widget never breaks a chat.

Sending:

- Bots use HTML parse mode with `<pre><code class="language-tgw">…</code></pre>`, with the JSON
  escaped for HTML.
- In the demo, the Widget Lab's "send to chat" button composes the text and the `pre` entity
  directly.

Conventions and limits:

- A message holds at most 4096 characters, so large templates ship bundled in the demo builds and
  are referenced by id and version.
- Widget identity is chat id plus message id until the backend assigns widget ids.
- Editing a widget message replaces the widget's state, and clients hide the "edited" label on
  widget messages.
- Forwarded widget messages are independent static copies in v0.
- The chat list and reply previews show the template's summary instead of the JSON.

## Document model

A widget is a template (structure, shared and cacheable) plus state (the data one message
carries), resolved against an environment the client supplies. The envelope carries the state and
either an inline template or a reference to a bundled one.

Envelope fields:

| Field           | Type    | Required       | Meaning                                                                             |
| --------------- | ------- | -------------- | ----------------------------------------------------------------------------------- |
| `tgw`           | integer | yes            | Envelope format version, `1` in v0                                                  |
| `template`      | object  | one of the two | Inline template                                                                     |
| `template_ref`  | object  | one of the two | `{"id": "tg.chess", "version": 1}`, looked up by the client among bundled templates |
| `state`         | object  | no             | The widget's data, `{}` when absent                                                 |
| `state_version` | integer | no             | Monotonic version, which v1's bot raises by one with every edit                     |
| `acks`          | list    | no             | The actions the bot processed since its previous edit, in v1                        |

Template fields:

| Field     | Type             | Required | Meaning                                                                             |
| --------- | ---------------- | -------- | ----------------------------------------------------------------------------------- |
| `spec`    | integer          | yes      | Spec version the template targets; crates reject newer specs                        |
| `id`      | string           | yes      | Reverse-DNS style id such as `tg.clock`                                             |
| `version` | integer          | yes      | Template revision                                                                   |
| `root`    | node             | yes      | The node tree                                                                       |
| `summary` | value            | no       | Plain text for the chat list, reply previews and notifications                      |
| `tick`    | string           | no       | `second` (default) or `minute`, the clock granularity when the tree reads `env.now` |
| `width`   | string or number | no       | `fill` (default) or a preferred width in units                                      |
| `defs`    | object           | no       | `symbols`: named vector drawings reused through `use` nodes                         |
| `actions` | object           | no       | Actions by name, in the order the keyboard lists them, in v1                        |
| `roles`   | object           | no       | Named boolean expressions over `state` and `actor`, in v1                           |
| `view`    | object           | no       | The view a viewer starts from, `{}` when absent, in v1                              |

Environment, read-only inside expressions:

| Name            | Meaning                                           |
| --------------- | ------------------------------------------------- |
| `env.now`       | Server-synced Unix time in milliseconds           |
| `env.tz_offset` | Viewer's current UTC offset in minutes            |
| `env.dark`      | `true` when the theme is dark                     |
| `env.rtl`       | `true` when the layout direction is right to left |
| `env.viewer`    | The viewer's `{id, name}`, or `null`, in v1       |

Values: every property takes either a literal JSON value or an expression written as
`{"=": "<expression>"}`. Inside a `repeat`, the current item and index are in scope under the
names the `repeat` declares.

Compatibility: unknown properties are ignored, so minor additions never break v0 crates. An
unknown node type is an error unless the node carries a `fallback` node, which renders in its
place.

## Node vocabulary

v0 has sixteen node types, exactly what the seven samples need and nothing speculative. Every node
accepts `key` (stable identity), `visible` (default `true`) and `a11y` (an accessibility label).

Containers:

| Type       | Children                        | Key properties                                                            | Notes                                                       |
| ---------- | ------------------------------- | ------------------------------------------------------------------------- | ----------------------------------------------------------- |
| `column`   | flow nodes                      | `gap`, `padding`, `align`, `background`, `radius`, `border`               | Stacks children top to bottom                               |
| `row`      | flow nodes                      | Same as `column`; each child sets `width` or `weight`                     | Splits the width across children                            |
| `scroll_x` | flow nodes with a fixed `width` | `gap`, `padding`                                                          | Horizontal scrolling strip, used for kanban lanes           |
| `canvas`   | canvas nodes                    | `view_box` `[w, h]`, `background`, `radius`                               | Scales a coordinate space to the available width            |
| `frame`    | one flow node, in `child`       | `x`, `y`, `w`, `h` in view-box units, plus the box properties of `column` | Places flow content inside a canvas                         |
| `repeat`   | one child template              | `each`, `as`, `index_as`, `key`                                           | Expands a list into children, allowed wherever its child is |

Flow leaves:

| Type         | Key properties                                                                     | Notes                                                                               |
| ------------ | ---------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------- |
| `text`       | `text`, `entities`, `style`, `color`, `align`, `max_lines`, `tabular`              | `entities` use Bot API `MessageEntity` offsets; `tabular` forces fixed-width digits |
| `text_field` | `label`, `value`, `placeholder`, `lines`                                           | An input-styled box, edited in v1 through a native sheet                            |
| `chart`      | `kind` (`line` or `bar`), `labels`, `series`, `y_min`, `y_max`, `height`, `legend` | Each series has `name`, `values` and `color`                                        |
| `divider`    | `color`                                                                            | One device pixel across the full width                                              |

Canvas nodes, with coordinates in view-box units:

| Type        | Geometry                                                                              | Notes                                                                                       |
| ----------- | ------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------- |
| `rect`      | `x`, `y`, `w`, `h`, `rx`                                                              | Rounded when `rx` is set                                                                    |
| `ellipse`   | `cx`, `cy`, `rx`, `ry`                                                                |                                                                                             |
| `line`      | `x1`, `y1`, `x2`, `y2`                                                                |                                                                                             |
| `path`      | `d`                                                                                   | SVG path data, absolute and relative: `M L H V C S Q T A Z`                                 |
| `use`       | `symbol`, `x`, `y`, `w`, `h`, `color`                                                 | Draws a `defs.symbols` entry scaled into the box; `current` inside the symbol means `color` |
| `connector` | `from`, `to`, `route` (`straight` or `orthogonal`), `arrow` (`end`, `both` or `none`) | Joins two `frame` keys; the crate turns it into paths                                       |

Canvas shapes share paint properties: `fill`, `stroke`, `stroke_width` (view-box units),
`opacity`, `dash` as `[on, off]`, `cap` and `join`.

Box properties: `padding` is a number, `[vertical, horizontal]` or `[top, end, bottom, start]`.
`align` is `start`, `center`, `end` or `stretch` on the cross axis, and `border` is
`{"color": …, "width": …}`.

v1 gives `on_tap`, `draggable`, `drop`, `actions` and `roles` their meaning under *Interactivity
(v1.0)*, and `reducers` stays reserved.

## Expressions

Expressions are pure and bounded: no loops, no assignment, no I/O, and cost linear in expression
size. A fuel limit caps the total work of every resolve.

Syntax:

- Literals: numbers, `'strings'` or `"strings"`, `true`, `false`, `null`, and lists such as
  `[1, 2]`.
- Paths: `state.lanes`, `env.now`, `item.title`, and indexing as `list[0]` or `map['key']`.
- Operators, tightest first: unary `!` and `-`; `* / %`; `+ -`; `< <= > >=`; `== !=`; `&&`;
  `||`; `?:`.
- `+` concatenates when either side is a string.

Types are JSON's: null, boolean, number (64-bit float), string, list and map. Comparing different
types yields `false`, and arithmetic on non-numbers yields `null`.

| Function                                    | Result                                                                      |
| ------------------------------------------- | --------------------------------------------------------------------------- |
| `len(x)`                                    | Length of a string in characters, or of a list or map                       |
| `range(n)`                                  | `[0, 1, …, n - 1]`, capped by the budgets                                   |
| `floor(x)`, `ceil(x)`, `round(x)`, `abs(x)` | Rounding and absolute value                                                 |
| `min(a, b)`, `max(a, b)`                    | Smaller or larger number                                                    |
| `string(x)`                                 | Text form; whole numbers print without a decimal point                      |
| `pad(x, width)`                             | A number left-padded with zeros                                             |
| `format_number(x, digits)`                  | Fixed decimals with `.` as separator in v0                                  |
| `format_time(ms, pattern)`                  | Local time using `env.tz_offset`; tokens `yyyy MM M dd d HH H hh h mm ss a` |

Errors: a failing expression yields `null`, and the property falls back to its default. One bad
value never fails a resolve; only exhausted budgets do.

Time: reading `env.now` marks the tree as time-dependent. The crate then reports the next wake-up
at the next `tick` boundary, and the client resolves again only while the widget is visible.

## Theme: colors, text styles, units

Colors are theme tokens by default, so a theme switch is a repaint with no crate call. Clients map
each token to their existing palette keys, separately for incoming and outgoing bubbles.

| Token                             | Meaning                                                                                                  |
| --------------------------------- | -------------------------------------------------------------------------------------------------------- |
| `text`                            | Primary text on the bubble                                                                               |
| `text_secondary`                  | Secondary text and captions                                                                              |
| `accent`                          | The bubble's accent, as used for links and active elements                                               |
| `on_accent`                       | Content drawn on top of `accent`                                                                         |
| `surface`                         | Widget panels inside the bubble                                                                          |
| `surface_alt`                     | Nested panels and alternate cells                                                                        |
| `divider`                         | Separators and borders                                                                                   |
| `positive`, `negative`, `warning` | Status colors                                                                                            |
| `palette_1` to `palette_7`        | Telegram's peer colors (red, orange, violet, green, cyan, blue, pink) with their light and dark variants |

Modifiers: `accent@0.4` sets the alpha to 0.4. Fixed colors `#RRGGBB` and `#AARRGGBB` are allowed
where the color carries meaning, such as chess pieces, and they do not adapt.

Text styles map to each client's own fonts: `body` (message text size), `caption`, `title`
(semibold), `display` (large numerals) and `mono`.

Units: sizes are in `u`, one density-independent pixel (dp on Android, a logical pixel times the
interface scale on desktop). Canvas coordinates use view-box units instead, scaled to the canvas
width.

## Layout

Layout is written per client and kept deliberately small: widths flow down, heights flow up, and
each text is measured once at a known width. v0 has no intrinsic-width negotiation, so every
client needs a single measuring pass.

Rules for an available width `W`:

| Node         | Width given to children                                                                          | Own height                                                               |
| ------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------ |
| `column`     | `W` minus horizontal padding, capped by a child's own `width`                                    | Sum of children, gaps and vertical padding                               |
| `row`        | Fixed-`width` children first, then the rest split by `weight` (default 1) after gaps and padding | Tallest child plus vertical padding                                      |
| `scroll_x`   | Each child's own `width`, which is required                                                      | Tallest child plus vertical padding; content scrolls when wider than `W` |
| `canvas`     | Scale `s = W / view_box[0]`                                                                      | `view_box[1] × s`                                                        |
| `frame`      | `w × s` for its child                                                                            | Fixed at `h × s`; content aligns by `align` and clips                    |
| `text`       | —                                                                                                | Measured at `W` with its `style` and `max_lines`                         |
| `text_field` | —                                                                                                | Label plus a box of at least `lines` lines with 10u padding              |
| `chart`      | —                                                                                                | `height` in units, default `0.6 × W`                                     |
| `divider`    | —                                                                                                | One device pixel plus 4u above and below                                 |

Cross-axis `align` positions children, and `stretch` gives a row's children the row's inner height
for their background and border. Text inside a frame keeps its unit size; only geometry scales with
the canvas.

Right to left: when `env.rtl` is true, `row` and `scroll_x` reverse child order and `start` and
`end` swap sides. Canvas contents never mirror, so boards and diagrams stay correct.

Widget width: `fill` takes the bubble's content width, and a number is a preferred width capped by
it. On TDesktop this maps onto `countOptimalSize()` and `countCurrentSize(width)`, and on Android
onto the cell's measure pass.

Paint order: children paint in order, backgrounds before content, and every container clips to
its bounds.

## Resolved tree

The crate hands clients a resolved tree: every expression evaluated, every default filled in, and
every convenience node expanded into primitives. Clients never evaluate expressions, never apply
defaults, and never see `repeat`, `use` or `connector`.

What resolution does:

- Expands `repeat` into keyed children; unkeyed nodes get positional keys such as `0/2/1`.
- Replaces `use` with the symbol's shapes plus an affine `transform` `[a, b, c, d, e, f]`, and
  substitutes `current` with the node's `color`.
- Turns `connector` into `path` nodes: a straight or orthogonal line between frame edges, plus an
  arrowhead.
- Normalizes path data to absolute `M`, `L`, `C` and `Z`, so clients implement four path commands.
- Drops invisible nodes and writes every layout property explicitly.
- Keeps colors as tokens, so a theme change never needs a resolve.

Encoding in v0 is deterministic UTF-8 JSON, `{"tgw_resolved": 1, "width": …, "root": …}`, with a
fixed key order and shortest round-trip numbers. Trees are small and resolve only on state changes
and clock ticks, and a binary encoding can replace JSON later behind the same format number.

Identical output bytes mean an unchanged tree, so clients skip layout. Stable keys let clients
animate nodes that moved between two resolves.

Node types clients implement: `column`, `row`, `scroll_x`, `canvas`, `frame`, `text`,
`text_field`, `chart`, `divider`, `rect`, `ellipse`, `line` and `path`.

## Budgets and failure behavior

The crate enforces hard budgets while parsing, not after, because every client parses widgets from
strangers just by opening a chat. Any violation fails the widget, and the client shows the message
as ordinary text.

| Budget                        | Initial limit                      |
| ----------------------------- | ---------------------------------- |
| Inline envelope               | 4096 characters, the message limit |
| Template bytes                | 256 KB                             |
| State bytes                   | 64 KB                              |
| JSON nesting depth            | 64                                 |
| Template nodes                | 2,000                              |
| Resolved nodes                | 10,000                             |
| Resolved tree size            | 1 MB as written                    |
| Items per `repeat`            | 1,000                              |
| Path commands across the tree | 20,000                             |
| String length                 | 8 KB                               |
| Expression length             | 1 KB                               |
| Expression nesting            | 32                                 |
| Evaluation fuel per resolve   | 200,000 steps                      |
| View-box dimensions           | 1 to 10,000                        |

These numbers are starting points; tune them with Widget Lab profiling.

Failure behavior:

- Parse, schema, budget or version errors render the message as ordinary text.
- A failing expression yields `null`, and the property takes its default.
- Exhausting fuel, the resolved-node budget or the resolved-tree size during a resolve also
  renders the message as ordinary text.

## Crate C API

The C API mirrors tlottie: opaque handles, UTF-8 byte slices in, status codes out, and
caller-provided output buffers. The full header, `twidget.h`, ships as a separate file.

| Function                                        | Purpose                                                                      |
| ----------------------------------------------- | ---------------------------------------------------------------------------- |
| `twidget_abi_version`                           | Checked once at startup against `TWIDGET_ABI_VERSION`                        |
| `twidget_envelope_template_ref`                 | Reads an envelope's `template_ref` without building anything                 |
| `twidget_template_new`, `twidget_template_drop` | Parses and validates a template once; immutable and shareable across threads |
| `twidget_instance_new`, `twidget_instance_drop` | Binds an envelope to its inline or bundled template                          |
| `twidget_instance_set_state`                    | Replaces state when the message is edited                                    |
| `twidget_resolve`                               | Writes the resolved tree and the next wake-up time                           |
| `twidget_summary`                               | Writes the summary text for previews                                         |
| `twidget_last_error`                            | Thread-local error detail for logs                                           |

Rules:

- Constructors return NULL on failure and report a status, and drop functions accept NULL.
- An instance holds its own reference to its template, so callers may drop templates freely.
- Instances are not thread-safe, so callers serialize access to each one.
- A too-small buffer returns `TWIDGET_ERROR_BUFFER_TOO_SMALL` with the required size, and the
  retry reuses the cached result.
- The crate has no threads, clocks, callbacks or I/O; the client passes time in through
  `TWidgetEnv`.
- A caught panic returns `TWIDGET_ERROR_INTERNAL` and sets the last error, so a widget bug never
  crashes an app built with unwinding.

v1 adds dispatch, buttons, edits with acknowledgements and the view, under *Interactivity
(v1.0)*; patches stay reserved for v2.

## Client integration

Each client adds four pieces: detection, a widget view, a token map and a Widget Lab. App changes
happen in fresh clones with Claude Code, driven by prompts written from this spec.

TDesktop:

- Detect when a message view is built: a `pre` entity with language `tgw` and an envelope the
  crate accepts.
- Render through a `HistoryView::Media` subclass, sized by `countOptimalSize()` and
  `countCurrentSize(width)`.
- Paint with `Painter`, draw text with `Ui::Text::String`, and map tokens to the message style's
  incoming and outgoing colors.
- Arm a `base::Timer` at the crate's wake-up time only while the view is visible.
- Link the crate like `lib_lottie` links tlottie, and move heavy parsing off the main thread with
  `crl::async` and `crl::on_main` if profiling asks.

Android:

- Add a widget message type in `MessageObject`, drawn by `ChatMessageCell` on its canvas.
- Draw text with `StaticLayout`, and map tokens through `Theme` keys for incoming and outgoing
  bubbles.
- Hold the native handle as a `long` in one Java class with one JNI file, as `RLottieNative` and
  `lottie.cpp` do.
- Link a prebuilt `twidget` static library per ABI, following tlottie's prebuilt path.
- Schedule re-resolves on the UI thread only while the cell is attached.

Both clients:

- Ship bundled templates as resources at `tgw/templates/<id>@<version>.json`.
- Use `twidget_summary` for the chat list and reply previews.
- Add the Widget Lab debug screen: every fixture in incoming and outgoing bubbles, toggles for dark
  theme, RTL, width and frozen time, and a "send to chat" button.

## The seven samples

All seven fit the v0 vocabulary without special cases, and each becomes a fixture in the crate
repository. Chess and kanban use bundled templates; the other five fit inline. An eighth,
conformance-only fixture, `chess_start`, holds the full starting position so every piece symbol is
tested.

| Sample         | Template             | Structure                                                                                      | State                                                               | Shows                                          |
| -------------- | -------------------- | ---------------------------------------------------------------------------------------------- | ------------------------------------------------------------------- | ---------------------------------------------- |
| Digital clock  | `tg.clock`, inline   | `column` with a `display` text bound to `format_time(env.now, 'HH:mm:ss')`                     | `label`                                                             | Local ticks with no network                    |
| Vector shapes  | `tg.shapes`, inline  | `canvas` of `ellipse`, `rect`, `path` and `use`, all in tokens                                 | none                                                                | Instant recolor on theme switch                |
| Graph          | `tg.chart`, inline   | `column` with a title `text` and a `chart` bound to state                                      | `title`, `labels`, `series`                                         | Theme-aware series, live when the author edits |
| Flowchart      | `tg.flow`, inline    | `canvas` with a `repeat` of nodes as `frame`s and a `repeat` of edges as `connector`s          | `nodes` (`id`, `x`, `y`, `w`, `h`, `label`), `edges` (`from`, `to`) | Author-computed layout, resolved connectors    |
| Kanban         | `tg.kanban`, bundled | `scroll_x` of lane `column`s, each with a `repeat` of card `column`s                           | `lanes` with `title` and `cards`                                    | Horizontal lanes, RTL mirroring                |
| Chess          | `tg.chess`, bundled  | 8×8 `canvas`: a `repeat` of squares, last-move highlights, a keyed `repeat` of pieces as `use` | `white`, `black`, `turn`, `last`, `pieces`                          | Symbols, a board that never mirrors            |
| Custom textbox | `tg.textbox`, inline | `column` with a caption and a `text_field`                                                     | `label`, `text`, `placeholder`                                      | Input styling; editing arrives in v1           |

Digital clock, as a complete envelope:

```json
{
  "tgw": 1,
  "template": {
    "spec": 1,
    "id": "tg.clock",
    "version": 1,
    "tick": "second",
    "width": 240,
    "summary": { "=": "state.label + ' ' + format_time(env.now, 'HH:mm')" },
    "root": {
      "type": "column",
      "padding": 16,
      "gap": 4,
      "align": "center",
      "background": "surface",
      "radius": 12,
      "children": [
        {
          "type": "text",
          "style": "display",
          "tabular": true,
          "align": "center",
          "text": { "=": "format_time(env.now, 'HH:mm:ss')" }
        },
        {
          "type": "text",
          "style": "caption",
          "color": "text_secondary",
          "align": "center",
          "text": { "=": "state.label" }
        }
      ]
    }
  },
  "state": { "label": "Local time" }
}
```

Chess, the bundled template `tg.chess@1`. The demo build defines all six piece symbols; two appear
here, and the final piece art must be license-compatible.

```json
{
  "spec": 1,
  "id": "tg.chess",
  "version": 1,
  "summary": { "=": "'Chess: ' + state.white + ' vs ' + state.black" },
  "defs": {
    "symbols": {
      "P": {
        "view_box": [45, 45],
        "shapes": [
          {
            "type": "ellipse",
            "cx": 22.5,
            "cy": 15,
            "rx": 6,
            "ry": 6,
            "fill": "current",
            "stroke": "#1B1B1B",
            "stroke_width": 1.5
          },
          {
            "type": "path",
            "d": "M14 37 L31 37 L27.5 23 L17.5 23 Z",
            "fill": "current",
            "stroke": "#1B1B1B",
            "stroke_width": 1.5
          },
          {
            "type": "rect",
            "x": 11,
            "y": 35,
            "w": 23,
            "h": 5,
            "rx": 2,
            "fill": "current",
            "stroke": "#1B1B1B",
            "stroke_width": 1.5
          }
        ]
      },
      "K": {
        "view_box": [45, 45],
        "shapes": [
          {
            "type": "path",
            "d": "M13 38 L32 38 L29 20 L16 20 Z",
            "fill": "current",
            "stroke": "#1B1B1B",
            "stroke_width": 1.5
          },
          {
            "type": "rect",
            "x": 21,
            "y": 5,
            "w": 3,
            "h": 13,
            "fill": "#1B1B1B"
          },
          {
            "type": "rect",
            "x": 17,
            "y": 9,
            "w": 11,
            "h": 3,
            "fill": "#1B1B1B"
          },
          {
            "type": "rect",
            "x": 10,
            "y": 36,
            "w": 25,
            "h": 5,
            "rx": 2,
            "fill": "current",
            "stroke": "#1B1B1B",
            "stroke_width": 1.5
          }
        ]
      }
    }
  },
  "root": {
    "type": "column",
    "gap": 8,
    "children": [
      {
        "type": "row",
        "gap": 8,
        "align": "center",
        "children": [
          {
            "type": "text",
            "style": "title",
            "text": { "=": "state.white + ' vs ' + state.black" }
          },
          {
            "type": "text",
            "style": "caption",
            "color": "text_secondary",
            "align": "end",
            "width": 110,
            "text": {
              "=": "state.turn == 'w' ? 'White to move' : 'Black to move'"
            }
          }
        ]
      },
      {
        "type": "canvas",
        "view_box": [8, 8],
        "radius": 8,
        "children": [
          {
            "type": "repeat",
            "each": { "=": "range(64)" },
            "as": "i",
            "child": {
              "type": "rect",
              "x": { "=": "i % 8" },
              "y": { "=": "floor(i / 8)" },
              "w": 1,
              "h": 1,
              "fill": {
                "=": "(i % 8 + floor(i / 8)) % 2 == 0 ? 'surface_alt' : 'accent@0.45'"
              }
            }
          },
          {
            "type": "repeat",
            "each": { "=": "state.last" },
            "as": "sq",
            "child": {
              "type": "rect",
              "x": { "=": "sq[0]" },
              "y": { "=": "sq[1]" },
              "w": 1,
              "h": 1,
              "fill": "warning@0.35"
            }
          },
          {
            "type": "repeat",
            "each": { "=": "state.pieces" },
            "as": "p",
            "key": { "=": "p.id" },
            "child": {
              "type": "use",
              "symbol": { "=": "p.kind" },
              "x": { "=": "p.x" },
              "y": { "=": "p.y" },
              "w": 1,
              "h": 1,
              "color": { "=": "p.side == 'w' ? '#FFFFFF' : '#3A3A3A'" }
            }
          }
        ]
      }
    ]
  }
}
```

A chess message then carries only the reference and the state. Board coordinates start at `x = 0`
on the a-file and `y = 0` on rank 8.

```json
{
  "tgw": 1,
  "template_ref": { "id": "tg.chess", "version": 1 },
  "state": {
    "white": "Mehran",
    "black": "Ali",
    "turn": "b",
    "last": [
      [4, 6],
      [4, 4]
    ],
    "pieces": [
      { "id": "wK", "kind": "K", "side": "w", "x": 6, "y": 7 },
      { "id": "bK", "kind": "K", "side": "b", "x": 4, "y": 0 },
      { "id": "wP5", "kind": "P", "side": "w", "x": 4, "y": 4 },
      { "id": "bP4", "kind": "P", "side": "b", "x": 3, "y": 1 }
    ]
  }
}
```

## Conformance testing

Conformance has three layers, and the seven fixtures drive all of them. A fixture is an envelope
plus environment values and widths, with expected outputs checked in beside it.

| Layer            | Checks                                                                                        | Runs in                            |
| ---------------- | --------------------------------------------------------------------------------------------- | ---------------------------------- |
| Resolution       | Envelope to resolved tree JSON, byte for byte                                                 | Crate CI                           |
| Reference layout | Resolved tree to frames at widths 240, 320 and 480, with fake text metrics                    | Crate CI through the `cli` feature |
| Client layout    | Each client's native layout against the same expected frames, with the fake measurer injected | TDesktop and Android test targets  |

Fake text metrics make layout deterministic. Each character advances 0.55 × font size (0.6 for
`mono`), a line is 1.25 × font size tall, and lines break greedily at spaces. Font sizes are 14u
for `body`, 12u for `caption`, 16u for `title`, 32u for `display` and 13u for `mono`.

The CLI also renders any fixture to SVG through the reference layout, so authors can preview a
widget without building a client. Checking real rendering stays manual in the Widget Lab for v0.

## Roadmap after v0

v1 adds shared interactivity and v2 moves the source to the backend, after the marketing demo.
Names reserved now keep every v0 envelope valid later.

| Stage | Adds                                                                                                | Source                                                                    |
| ----- | --------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------- |
| v0    | Rendering, theme, RTL, clock ticks, the seven samples                                               | Plain-text messages                                                       |
| v1    | Actions, reducers in the crate, drag and drop, text editing through native sheets, roles and guards | Plain text plus a stand-in bot that applies actions and edits the message |
| v2    | Widget ids, a template cache keyed by hash, pushed state patches, server-side reducers              | MTProto                                                                   |

v1 gives reserved names their meaning: the node properties `on_tap`, `draggable` and `drop`, the
template fields `actions` and `roles`, the envelope field `acks` and the code block language
`tgw-action`. The template field `reducers` and the envelope field `actor` stay reserved.

## Decisions log

Ten decisions shape v0; each row names the choice and the reason it was made.

| Decision             | Choice                                                                       | Reason                                                                           |
| -------------------- | ---------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| Shared or per client | The crate interprets; clients lay out and paint                              | Matches tlottie, tde2e and tgcalls on one side, math and markdown on the other   |
| Crate language       | Rust with a C ABI                                                            | tlottie set the precedent, the build plumbing exists, and the input is untrusted |
| Linking              | Android: a self-contained archive; Desktop: umbrella with tlottie, if agreed | Android links beside other Rust libraries today; an umbrella would link std once |
| v0 transport         | A `tgw` code block inside a text message                                     | Needs no backend, gives exact boundaries and a readable fallback                 |
| Templates in v0      | Inline, or bundled and referenced by id and version                          | The 4096-character message limit                                                 |
| Crate output         | Resolved tree as deterministic JSON                                          | Small trees and easy debugging; binary later if profiling asks                   |
| Layout               | Widths down, heights up, no intrinsic widths                                 | One measuring pass that fits both clients' message layout                        |
| TDLib                | Out of scope                                                                 | Nothing stops third-party clients from linking the crate later                   |
| App work             | Fresh clones with Claude Code, prompts from this spec                        | Keeps app changes out of the design conversation                                 |
| Spec location        | `docs/SPEC.md` in the crate repository leads, and approved answers land here | One source of truth for Claude Code and the apps                                 |

## Implementation clarifications (v0.1)

These answers settle the gaps found while planning the crate. Where this section is more specific
than the sections above, it governs, and each subsection names the milestone that first needs it.

### Tooling and repository (M0.1)

- Tests may panic. `clippy.toml` enables the `allow-*-in-tests` settings the pinned clippy supports
  for `unwrap_used`, `expect_used`, `panic` and `indexing_slicing`. Each file under `tests/` may
  carry one crate-level `#![allow]` for those four lints, with a comment saying tests fail by
  panicking. Library code never does either.
- `rustfmt.toml` sets `edition = "2021"`, `max_width = 100`, `tab_spaces = 4` and
  `newline_style = "Unix"`. The nightly-only import options stay out, so `cargo fmt --check` runs
  clean on stable, and imports are grouped by hand.
- `license` stays unset and `publish = false` until the team decides. v0 ports no third-party
  code, and the chess silhouettes are drawn from scratch.

### Fixtures (M0.2)

- An eighth, conformance-only fixture, `chess_start`, holds the full starting position, so all six
  piece symbols reach the goldens and SVGs. It is not a demo sample.
- With it, the goldens become 32 resolved trees (8 fixtures, light and dark, LTR and RTL), 24 frame
  files (8 fixtures at widths 240, 320 and 480) and 8 RTL frame files at 320.
- A `frame` holds its single flow node in `child`, as `repeat` does.
- The `chart` fixture puts a `divider` between its title and its chart, so every node type appears
  in at least one fixture.

### JSON (M1.1)

- Numbers are written the way RFC 8785 (the JSON Canonicalization Scheme) writes them: shortest
  round-trip digits in ECMAScript notation. So `1e21` is written `1e+21`, `1e-7` keeps its
  exponent, and `-0` is written `0`.
- Non-finite numbers are rejected on input and never produced.
- The 4,096-character envelope budget counts UTF-16 code units, Telegram's unit for text offsets.
  KB means 1,024 bytes, and string lengths are bytes of decoded UTF-8.
- Duplicate keys fail with `TWIDGET_ERROR_JSON`.

### Document model (M1.2)

- `{"=": "…"}` is an expression only as a whole property value, including `repeat.each` and
  `repeat.key`. Inside literal arrays and objects it is plain data, and state is never evaluated.
- `defs.symbols` holds only `rect`, `ellipse`, `line` and `path` with literal values; `fill` and
  `stroke` may be `current`.
- Invalid literal path data fails the load with `TWIDGET_ERROR_SCHEMA`. Invalid path data produced
  by an expression leaves `d` empty, and empty paths are dropped from the resolved tree.
- One path command is one drawing segment after implicit repeats expand, so `L 1 2 3 4` counts as
  two. The 20,000 limit applies to literal paths at load and again to normalized paths at resolve.
- Every node object in the template counts toward the 2,000 nodes, symbol shapes and fallback
  subtrees included.
- A template `id`, and the `id` in an envelope's `template_ref`, is 3 to 64 bytes of `a-z`,
  `0-9`, `.`, `_` and `-`, starts and ends with a letter or digit, contains at least one dot and
  never `..`. Apps turn it into a resource file name, so this is a path-safety rule.
- `fill`, `stroke` and `background` also accept `none`, which paints nothing. It takes no
  `@alpha`, and it resolves to `null`.
- A `canvas` and each symbol need a `view_box`, a `use` needs a `symbol` and a `repeat` needs a
  `child`; a template missing one fails with `TWIDGET_ERROR_SCHEMA`. A `frame` may leave out its
  `child`. A literal view-box dimension outside 1 to 10,000 fails with `TWIDGET_ERROR_BUDGET`, as
  the budget table says.
- The root and a frame's `child` hold exactly one node, so a `repeat` in either place fails the
  load with `TWIDGET_ERROR_SCHEMA`; a `repeat` belongs among a container's `children`.

### Expressions (M2.1)

- Logic is strictly boolean. `!`, `&&`, `||` and the condition of `?:` take booleans and yield
  `null` for anything else; `&&` and `||` short-circuit and return booleans.
- `+` with a string on either side converts the other side with `string()`, and `+` with a list
  or map yields `null`. `==` and `!=` compare lists and maps structurally, spending one fuel step
  per element. `!=` is always the negation of `==`, so it yields `true` for different types.
- `<`, `<=`, `>` and `>=` compare two numbers numerically and two strings by Unicode scalar
  values. Any other pair yields `false`, as different types do, so they always yield a boolean.
- `round` rounds ties away from zero. `format_number(x, d)` clamps a whole `d` to 0–10, yields
  `null` for a fractional one, and formats the exact binary value with ties to even, as Rust's
  `{:.d$}` does. Any non-finite result yields `null`.
- `range(n)` takes a whole number from 0 to 1,000. Above 1,000 it fails the resolve with
  `TWIDGET_ERROR_BUDGET`, and a fractional or negative `n` yields `null`.
- `len` counts Unicode scalar values. `string()` writes numbers as the JSON writer does, `null` as
  an empty string and booleans as `true` or `false`, and yields `null` for lists and maps.
- `pad(x, w)` takes a whole number and a width from 0 to 20 and puts the sign before the zeros, so
  `pad(-5, 3)` is `-005`; fractions and widths outside 0 to 20 yield `null`.
- `format_time` floors a fractional `ms`, writes `a` as `AM` or `PM`, copies every non-token
  character literally, and always reads token letters as tokens, longest first. A letter that
  starts no token, such as each `y` of `yy` or a lone `m` or `s`, is copied literally.
- Strings accept the escapes `\\`, `\'`, `\"`, `\n`, `\t` and `\uXXXX`. Number literals are
  decimal, need a leading digit, and may have a fraction and an exponent.
- Without `as` and `index_as`, a `repeat` binds `item` and `index`. Binding `state` or `env` is a
  schema error, and inner bindings shadow outer ones.
- An expression that fails to parse fails the template load with `TWIDGET_ERROR_SCHEMA`, and so
  does a call to an unknown function, a call with the wrong number of arguments, or a name that
  is neither `state`, `env` nor a binding in scope.
- Nesting counts levels. Parentheses, a list, a call, a unary operator and a `?:` each hold their
  operands one level deeper, as does a run of binary operators of one precedence (`a + b - c`) or
  of `.name` and `[index]` steps (`state.lanes[0].title`), each run taken as a whole. Literals and
  names are at level 0; 32 levels pass, and 33 fail with `TWIDGET_ERROR_BUDGET`.
- Fuel costs one step per evaluated syntax node, plus one per list element or character produced.
- A string an operator or function builds past the 8 KB string budget is a failing expression and
  yields `null`, since one bad value never fails a resolve.

### Resolver (M2.2 to M2.4)

- Resolved nodes list `type`, then `key`, then properties in the order of the M1.2 property
  tables, under the template's property names. Values only layout can settle are `null`: chart
  `height` (0.6 × W), an unset `width`, unlimited `max_lines`, and a missing fill or stroke. The
  crate documents this schema in `docs/RESOLVED.md`, part of the contract since v0.5.
- Shape defaults: `fill` is `text` on `rect`, `ellipse` and `path`, and absent on `line`; `stroke`
  is absent except on `line`, where it is `text`; `stroke_width` 1, `opacity` 1, `cap` `butt`,
  `join` `miter`, no dash.
- Text defaults: `style` `body`, `color` `text`, `align` `start`, unlimited `max_lines`, `tabular`
  false, no entities.
- Box defaults: `gap` 0, `padding` 0, `align` `start`, no background, `radius` 0, no border.
  `text_field.lines` defaults to 1.
- Other defaults: `use.color` is `text` and `divider.color` is `divider`; a `border` missing its
  `color` or `width` takes `divider` or 1; `text.text` and the `text_field` strings are empty;
  chart `labels` and `series` are empty, and so are a series' missing `name` and `values`; `a11y`
  is `null`. Every coordinate and size (`x`, `y`, `w`, `h`, `rx`, `ry`, `cx`, `cy`, `x1`, `y1`,
  `x2`, `y2`) is 0, so a shape without a size paints nothing, as in SVG, and a missing `d` is
  empty.
- Chart defaults: `kind` `line`, and `legend` when there are two or more series. Series colors
  follow `accent`, `palette_4`, `palette_2`, `palette_3`, `palette_5`, `palette_7`, `palette_1`,
  repeating, and non-numeric values become gaps.
- The crate writes an explicit y range: data minimum to maximum for `line` (widened by 1 on each
  side when flat), the same range stretched to include 0 for `bar`, and 0 to 1 without data. A
  `y_min` or `y_max` the template sets replaces that end, unless the range would then end at or
  below its start, in which case the computed range stands.
- Keys are full paths. The root is `0`, a child's segment is its index among its parent's template
  children, and each `repeat` item adds `:` plus its explicit key or its item index, so a chess
  piece is `0/1/2:wK`. Explicit keys have `/`, `:` and `#` replaced by `_`, and repeated keys
  within one `repeat` get `#2`, `#3` and so on in order.
- A node's own `key` replaces its index segment, sanitized the same way, and the root's replaces
  `0`. Inside a `repeat`, an item's key is the `repeat`'s `key`, else its child's own `key`, else
  its index. A segment that repeats among one parent's children gets `#2`, `#3` and so on in
  order, as repeated item keys do.
- A key, or a connector's `from` or `to`, may also be a number, which stands for the text
  `string()` writes for it.
- A `repeat` evaluates its `key` once per item, with the item's names in scope; its `each`,
  `visible` and `a11y` see only the names around it. `visible` false drops every item, and its
  `a11y` is ignored, since a `repeat` resolves to no node of its own.
- The crate validates `entities` against the resolved text in UTF-16 code units and clips them, so
  clients receive safe offsets.
- A `scroll_x` child whose `width` resolves to `null` is dropped with its subtree. A `repeat` whose
  `each` is not a list produces no children. A `canvas` whose `view_box` resolves to anything but
  two numbers from 1 to 10,000 is dropped with its subtree.
- `use` fits the symbol's view box into its box, keeping the aspect ratio and centered. An unknown
  literal symbol name fails the load with `TWIDGET_ERROR_SCHEMA`, and one produced by an
  expression drops the node.
- A `use`'s shapes take its key plus `/` and each shape's own key or its index in the symbol, and
  the first of them takes the `use`'s `a11y` unless it has its own.
- `straight` connectors join the two frames' centers, clipped at each frame's edge. `orthogonal`
  connectors leave and enter through the facing edge midpoints along the dominant axis, bending
  twice at the halfway line, and fall back to `straight` when the facing edges overlap.
- Arrowheads are filled triangles 4 × `stroke_width` long and 3 × `stroke_width` wide, in the
  stroke color. Connector defaults: `route` `straight`, `arrow` `end`, `stroke` `text_secondary`,
  `stroke_width` 1.5. A connector whose `from` or `to` matches no frame, or whose clipped segment
  has no length, is dropped.
- A connector's `from` and `to` name frames of the same canvas by the key the template gave them,
  the `repeat`'s item key or the frame's own `key`, both sides sanitized as keys are; when two
  frames share a key, the first drawn wins.
- A connector resolves to its line, keyed with `/0` after its own key, then an arrowhead at `to`
  keyed `/1` and one at `from` keyed `/2`, as `arrow` asks. The line takes the connector's
  `a11y` and stops at each arrowhead's base. An end segment shorter than its arrowhead is left
  out, and a line with no segment left is left out entirely.
- Arcs use no platform trigonometry. Center and radii follow SVG 1.1 appendix F.6 with `sqrt`
  only, directions stay unit vectors instead of angles, the sweep splits at exact quarter turns,
  and each piece's control distance comes from half-angle formulas using `sqrt`.
- The x-axis rotation uses an in-crate sine and cosine: exact at multiples of 90°, otherwise
  reduced to 0°–45° and evaluated by a fixed polynomial of basic arithmetic. fdlibm is not ported.
- The fixed test environment is `now_ms` 1790415687123 (2026-09-26 09:41:27.123 UTC) with
  `tz_offset_minutes` 210 (+03:30).

### C API (M3.1)

- A caught panic returns the new `TWIDGET_ERROR_INTERNAL` (-10) and sets the last error. This
  header change is approved, and the ABI version stays 1 because nothing has shipped.
- A NULL input pointer with length 0 reads as empty input, and with a nonzero length it is
  `TWIDGET_ERROR_INVALID_ARGUMENT`.
- A NULL output buffer with capacity 0 is a size query, answered with
  `TWIDGET_ERROR_BUFFER_TOO_SMALL`, or `TWIDGET_OK` when the result is empty. A NULL output buffer
  with a nonzero capacity, or a NULL `out_len` or `id_len`, is `TWIDGET_ERROR_INVALID_ARGUMENT`.
- An envelope with an inline template passed together with a non-NULL `bundled` fails with
  `TWIDGET_ERROR_TEMPLATE_MISMATCH`.
- `struct_size` must be at least this header's `sizeof(TWidgetEnv)`. The cached retry compares
  fields one by one, never raw bytes, because C leaves padding uninitialized.

### Reference layout and CLI (M5.1, M5.2)

- Frames JSON is `{"tgw_frames": 1, "width": …, "height": …, "nodes": […]}` in paint order. Each
  node has `key`, `x`, `y`, `w` and `h` in units relative to the widget, and text nodes add
  `lines` with each line's text and position.
- A word wider than its line breaks between characters, and a character is a Unicode scalar value.
- A `text_field` is a `caption` label, a 4u gap, then a box with 10u padding, a 1u `divider`
  border and radius 8u.
- `divider` is 1u thick in the reference layout, and borders never inset content; padding does.
- Frames cover flow nodes, canvases and frames. Canvas shapes, including those a `use` or a
  `connector` draws, stay out of them, in view-box units.
- Each entry of `lines` is `{"text": …, "x": …, "y": …}`: the line's text and the top-left corner
  of its line box, relative to the widget as node boxes are. Boxes are reported unclipped, since
  painting clips to every container.
- The fake measurer ends a line at `\n`. Lines break greedily at spaces, and a break consumes that
  one space while other spaces count as characters. A word wider than its line starts a new line
  and splits where it overflows, with at least one character per line. Empty text has no lines
  and no height. A run of n characters fits a width when n × advance ≤ width + 1e-6, where the
  advance is the font size times 0.55, or 0.6 for `mono`.
- `max_lines` keeps the first lines. When it cuts text, the last kept line ends with `…` (U+2026),
  dropping characters from its end until the line fits.
- A `text_field`'s value, or its placeholder when the value is empty, is `body` text, and the box
  holds at least `lines` lines of it. Its `lines` are the label's lines followed by the value's
  or placeholder's. An empty label takes no space and no gap.
- A `divider`'s box is its whole 9u slot: 4u, the 1u line, then 4u.
- Inside a frame, `align` places the child on both axes within the padded box: `start` at the top
  start, `center` in the middle, `end` at the bottom end, and `stretch` fills the box.
- The root takes the widget's width, since its own `width` and `weight` only matter to a parent.
  In a column, `stretch` acts as `start`.
- Right to left, the nodes list keeps the resolved order and only positions mirror. `scroll_x`
  content starts at the right edge and overflows to the left at scroll offset 0. Flow content
  inside a frame follows the direction; only canvas geometry never mirrors.
- Negative widths and heights count as 0, and when a row's weights sum to 0 or less its weighted
  children get no width.
- The CLI takes `--templates <dir>` (default `fixtures/templates`), `--now` in milliseconds and
  `--tz` in minutes, both defaulting to the fixed test environment. Its light and dark palettes
  approximate Telegram's default themes and are defined once, and generated SVGs are not checked
  in.
- SVGs set text in the generic `sans-serif` family, and `mono` text in `monospace`, at the
  fake-metric sizes, with `title` in weight 600.

### Hardening (M6.1)

- The resolved tree writes at most 1 MB (1,048,576 bytes), counted as `twidget_resolve` writes
  it. The resolver adds up each node's written size as it builds the tree and fails with
  `TWIDGET_ERROR_BUDGET` as soon as the total passes the limit, so no resolve builds or writes
  more. Without it, one message could multiply its state across thousands of nodes: a
  4,096-character envelope wrote 31 MB, and a 64 KB state 543 MB.

### Release (M7.1, M7.2)

- This repository builds for macOS and Android only, both on the Mac. macOS gets one universal
  `libtwidget.a`, `aarch64-apple-darwin` and `x86_64-apple-darwin` joined by `lipo`, built with
  `MACOSX_DEPLOYMENT_TARGET=10.13` as Telegram Desktop's `prepare.py` builds tlottie.
- Android gets a `libtwidget.a` for each of `armeabi-v7a`, `arm64-v8a`, `x86` and `x86_64`, built
  with NDK 27.2.12479018 for API level 21, as Telegram Android's `TMessagesProj/build.gradle` sets
  them: `minSdkVersion 21`, `-DANDROID_PLATFORM=android-21` and `ndkVersion "27.2.12479018"`.
- Each Android `libtwidget.a` holds a single object whose only global symbols are the functions
  `include/twidget.h` declares. Every other definition, Rust std's personality routine and globals
  included, is local to it, and it keeps no section groups. So it links into one shared library
  beside other Rust static libraries, such as Telegram Android's tlottie and wallet engine, and its
  unwinding always runs its own personality routine. The macOS library stays as cargo builds it.
- Windows and Linux are deferred. They will come through a twidget stage in Telegram Desktop's
  `prepare.py`, cloned from its tlottie stage, and not through scripts here, so this repository
  has no Windows script; `prepare.py` runs its Windows steps as cmd batch. The Windows targets are
  `i686-win7-windows-msvc` and `x86_64-win7-windows-msvc`, built with
  `-Z build-std=std,panic_abort`, and `aarch64-pc-windows-msvc`, all with `+crt-static`, so
  `catch_unwind` cannot catch panics there and the lint policy is the only guarantee.
- Builds write to `prebuild/`, which this repository ignores: `prebuild/lib/<ABI>/libtwidget.a` for
  the four Android ABIs, `prebuild/lib/macos/libtwidget.a`, and `prebuild/BUILD_INFO`, which
  records the twidget commit, the rustc version, the NDK version and the API level, and for
  Android the number of global symbols, `exports=10`.

### Layout direction (Telegram Android M2.1)

- `env.rtl` gives the widget's own direction, taken from the text the widget draws rather than from
  the app, or from the message's fallback text, which patched clients hide. The client resolves
  with `env.rtl` false and finds the first strong character in the strings the resolved tree draws.
  When that character is right to left, it resolves again with `env.rtl` true and uses that tree;
  otherwise it keeps the first. When the tree draws no strong character, the app's layout
  direction decides, and a right-to-left app resolves again with `env.rtl` true.
- The strings drawn are, in the order the resolved tree lists its nodes, each node before its
  children: a `text`'s `text`; a `text_field`'s `label`, then its `value`, or its `placeholder` when
  the value is empty; and a `chart`'s `labels`, then its series' names when `legend` is true. Each
  string counts whole, before any `max_lines` cut. `a11y` labels are not drawn and do not count.
- A strong character is one of bidi class L, R or AL, found the way rules P2 and P3 of the Unicode
  Bidirectional Algorithm (UAX #9) find a paragraph's direction. Characters between an isolate
  initiator and its matching PDI are skipped, isolates match within each string, and R and AL mean
  right to left.
- The first resolve always uses `env.rtl` false, whatever the app's direction, so every client
  reaches the same direction for the same message.
- The client decides once per envelope, on its first resolve, and keeps that direction for clock
  ticks and theme changes. An edit brings a new envelope and decides again. The C ABI and the
  resolved format are unchanged, and the crate takes `env.rtl` as given.

### Scroll strip bleed (Telegram Android M2.1)

- A `scroll_x`'s content may draw past the strip's left and right edges, within the strip's own top
  and bottom, so slid content reaches the bubble's edges as Telegram's rich-message tables do. This
  is the one exception to *Layout*'s "every container clips to its bounds".
- On each side, the bleed's limit starts at the strip's edge and moves outward one ancestor at a
  time. An ancestor leaves a side free when it is a `column`, or a `row` or `scroll_x` in which the
  strip's branch is the outermost child on that side as laid out on screen. The limit then moves to
  that ancestor's outer edge on that side, padding and any space beside the branch included.
- The walk stops on a side at the first ancestor that does not leave it free, keeping the limit
  where it is. It stops on both sides at the first ancestor that paints a `background` or `border`
  or is a `frame`, and that ancestor's bounds, rounded by its `radius`, also clip the bleed.
- When the walk passes the widget's root on a side, the limit is the bubble's edge on that side.
  How far that edge is and the bubble's shape are the client's.
- Layout is unchanged: content starts where the layout puts it, and scroll offsets and frames JSON
  stay as they are. At offset 0, content overflowing the strip's far end shows in the free space
  beside it instead of being cut at the strip.
- The reference SVG preview still clips each strip at its own box, since a widget alone has no
  bubble.

### Previews (Telegram Android M2.7)

- The chat list, reply previews and notifications show the text `twidget_summary` writes. When that
  text is empty or only whitespace, they show the message's fallback text instead, which the author
  wrote for unpatched clients, such as "Kanban: 3 lanes". When the fallback text is empty too, the
  client shows a fixed label of its own, such as a localized "Widget".
- The fallback text is the message's text with the envelope's code block cut out, the text before
  the block and then the text after it, trimmed of whitespace at both ends. Later `tgw` blocks and
  other code blocks stay in it.
- A failing `twidget_summary` does not fall back: as for any failure, the preview shows the message
  as ordinary text.
- The client computes the summary when the message loads and again on an edit, since
  `twidget_summary` reports no wake-up, so a summary that reads `env.now`, such as the clock's,
  keeps its load time. A client may refresh it sooner.

### Animation (M8.1)

Templates may animate paint-only properties with keyframes and ease between trees with
transitions. The crate validates both, resolves their values with the tree and writes them once in
the resolved tree. Clients interpolate at draw time, only while the widget is visible, with no
crate call per frame. Animated templates keep `spec` 1: a crate without animation ignores the new
names, as it does unknown properties, and shows the resting frame.

Keyframes:

- Any node but `repeat` and `connector` may carry `animate`, a list of entries `{"property",
  "keyframes", "duration", "delay", "repeat", "origin"}`. `keyframes` lists 2 to 32 `{"at",
  "value", "easing"}`, where `at` is a literal fraction of the duration rising strictly from 0 to
  exactly 1.
- `duration` (1 to 60,000 ms, required), `delay` (0 to 60,000 ms, default 0) and each keyframe's
  `value` take a literal or an expression, so bars in a `repeat` can stagger by `index`.
  `property`, `at`, `easing`, `repeat` and `origin` are literal. A keyframe without `value` takes
  the node's own resolved value, so an intro ends exactly where the node rests.
- `easing` shapes the segment that starts at its keyframe: `linear` (default), `ease_in`,
  `ease_out`, `ease_in_out`, `hold`, which keeps the value until the next keyframe, or
  `[x1, y1, x2, y2]`, a cubic Bézier with both x values in 0 to 1. The named curves are CSS's:
  `[0.42, 0, 1, 1]`, `[0, 0, 0.58, 1]` and `[0.42, 0, 0.58, 1]`.
- `repeat` is `once` (default), `loop`, `alternate`, which plays forward and then backward, or a
  whole count from 1 to 1,000.
- `loop` and `alternate` run on server time, so every viewer sees the same frame: the position is
  `(server_now − delay) mod period`, the period being the duration, or twice it for `alternate`.
- `once` and counts start `delay` after the widget first appears in its chat view. Before that the
  node shows the first keyframe, and after the last cycle the last. An intro replays only when the
  message's view is built anew, not when it scrolls back into view or an edit arrives.

Properties:

- Every node may animate `opacity` (0 to 1, resting 1), `translate` (`[x, y]`, resting `[0, 0]`),
  `scale` (resting 1) and `rotate` (degrees clockwise, resting 0), which apply to the node with
  everything it draws. On flow nodes they exist only for animation; shapes keep their own
  `opacity`.
- Transforms apply around `origin`, fractions `[fx, fy]` of the node's box (default
  `[0.5, 0.5]`), in the order translate, rotate, scale, on top of the node's place and any symbol
  `transform`. A shape's box bounds its points, control points included. On flow nodes `translate`
  is in units and its x mirrors right to left; in a canvas it is in view-box units and never
  mirrors.
- Colors: `background` on `column`, `row`, `frame` and `canvas`, `color` on `text` and `divider`,
  and `fill` and `stroke` on shapes. A color interpolates per ARGB channel in sRGB after the client
  maps its tokens, and `none` stands for the other end's color at alpha 0.
- Geometry: `x`, `y`, `w`, `h` and `rx` on `rect`; `cx`, `cy`, `rx` and `ry` on `ellipse`; `x1`,
  `y1`, `x2` and `y2` on `line`; and `stroke_width` and `dash` on every shape. A number, and each
  member of a pair, moves as `a + (b − a) × e`, `e` being the eased progress.
- `count` on a `text` shows an integer moving between the keyframe values, rounded half away from
  zero, in place of the text. It applies only when `tabular` is true, the resolved text is an
  integer and no value has more digits than it; otherwise the entry is dropped. Layout measures
  the resolved text, so no frame needs a new layout.
- A `use` animates only `opacity` and the transforms, applied to each shape it draws.
- When a node has more than one entry for a property, the last in `animations` applies. A `use`'s
  entries come before each shape's own, so a symbol shape's own entry replaces its `use`'s.
- Width, padding, gap, text content, path data and every other property stay still, since
  animating them would need a layout or a new path each frame. A literal `property` the node's
  type cannot animate fails the load with `TWIDGET_ERROR_SCHEMA`.

Transitions:

- When a new tree replaces the old one after a state change, an edit or a clock tick, the client
  matches nodes by key and type and eases between them. Changed animatable properties move from
  old to new; a flow node or frame whose box moved draws offset from its old place, easing to
  none, while a size change applies at once; added nodes enter and removed nodes exit; and changed
  text changes in the node's text style.
- A shape's `transform` also moves from old to new, member by member, so a `use` that moves or
  resizes glides; a `use` only translates and scales, so this is exact.
- The template's `transition` is the default for every node, and a node's own `transition`
  replaces it for that node alone; `"none"` turns it off. Without a template `transition`, only
  nodes that ask transition.
- `repeat` and `connector` ignore `transition`, as they ignore `animate`. A `use`'s transition,
  like its entries, goes to each shape it draws, unless the symbol's shape sets its own.
- A transition is `{"duration", "easing", "enter", "exit", "text"}`, all literal: `duration` 1 to
  60,000 ms (default 250), `easing` as for keyframes but never `hold` (default `ease_in_out`),
  `enter` and `exit` `fade` (default) or `none`, and `text` `crossfade` (default), `roll_up`,
  `roll_down`, `count` or `none`.
- `roll_up` slides the old text up and out and the new text up from below, clipped to the node's
  box, and `roll_down` the reverse. When both texts are tabular and the same length, only the
  changed characters roll, each in its own cell; otherwise the whole text rolls. `count` moves
  between two integer texts under the `count` property's conditions, and crossfades otherwise.
- A tree that arrives mid-transition starts from what is on screen. The first tree, and a
  re-resolve for a theme, direction or width change, replace the tree without transitions. A
  running keyframe animation overrides the transitioned value of its property.

Reduced motion: when the system animation scale is 0 or the client's power saving is on, such as
Telegram's LiteMode, clients show every node's resting values and skip transitions.

Resolved tree:

- A top-level `motion` member follows `root`: `null` when nothing moves, else the default
  `transition`, the per-node `transitions` and the `animations`, each keyed by node, with every
  default written out, as `docs/RESOLVED.md` details. The format stays 1, and
  `include/twidget.h` is unchanged.
- Entries of invisible nodes are dropped, a node in a `repeat` yields one entry per item under its
  item key, and a `use`'s entries go to each shape it draws. Entries carry the keyframe values the
  crate filled in, named easings as Bézier arrays, `once` as 1, and `origin` as fractions of a
  flow node's or frame's box or as a view-box point for a shape.

Budgets:

| Budget                              | Initial limit                     |
| ----------------------------------- | --------------------------------- |
| Animation entries per resolved tree | 1,000                             |
| Keyframes per entry                 | 32                                |
| Duration and delay                  | 0 to 60,000 ms, durations from 1  |
| Repeat count                        | 1 to 1,000                        |

A literal out of range fails the load with `TWIDGET_ERROR_BUDGET`. A value an expression produces
out of range drops its entry, or for a keyframe takes the node's own value, and more than 1,000
resolved entries fail the resolve with `TWIDGET_ERROR_BUDGET`.

Conformance gains a fourth layer, sampled values:

- `twidget-cli sample <envelope> --at <ms> [--appear <ms>]` writes `{"tgw_sampled": 1, "at": …,
  "values": [{"key", "property", "value"}]}`: each entry's value at server time `at` for a widget
  that appeared at `appear`, which defaults to the resolve time. A color samples as `{"from",
  "to", "mix"}`, which the client mixes in its palette.
- The reference solves a Bézier's x by bisecting 40 times with basic arithmetic, and clients
  compare within 1e-6 of each entry's span.
- A ninth fixture, `motion`, has equalizer bars, rings drawn as arc paths with a count-up,
  floating reactions and a rolling clock digit. The goldens become 36 resolved trees, 27 frame
  files and 9 RTL frame files, plus its sampled values under `fixtures/expected/sampled/`.
- Transitions have no goldens: these rules are their reference, checked in the Widget Lab.

## Interactivity (v1.0)

v1 makes widgets interactive. A tap, a drag or a text field's submit runs actions the template
declares. A local action changes the viewer's own `view`, which stays on the device. A shared
action changes `state`: the app applies it at once and sends it to a bot, which applies it again
as the authority and writes the result back by editing the message. Both run the crate's
reducers, so the app's optimistic result and the bot's answer agree.

Interactive templates keep `spec` 1. A crate without v1 ignores the new names, as it does unknown
properties, and shows the widget as v0 does, with the bot's buttons under the message.

### Template members

| Field     | Type   | Meaning                                                         |
| --------- | ------ | --------------------------------------------------------------- |
| `actions` | object | The actions by name, in the order the bot's keyboard lists them |
| `roles`   | object | Named boolean expressions over `state` and `actor`              |
| `view`    | object | The view a viewer starts from, `{}` when absent                 |

- Action and role names are 1 to 32 bytes of `a-z`, `0-9` and `_`, starting with a letter.
- `view` is plain data, never evaluated, as `state` is.
- The template field `reducers` and the envelope field `actor` stay reserved, and v1 ignores
  them. The bot learns who acted from Telegram, so no payload names an actor that could be forged.

### Actions

A one-vote-per-user poll:

```json
"actions": {
  "vote": {
    "scope": "state",
    "args": { "option": "int" },
    "let": { "me": "string(actor.id)", "old": "state.votes[me]" },
    "guard": "args.option >= 0 && args.option < len(state.options) && old != args.option",
    "ops": [
      { "inc": "state.tally[old]", "by": "-1", "when": "old != null" },
      { "inc": "state.tally[args.option]" },
      { "set": "state.votes[me]", "to": "args.option" }
    ],
    "outcomes": "[{args: {option: i}, label: state.options[i]} for i in range(len(state.options))]"
  }
}
```

| Member     | Type                 | Required | Meaning                                             |
| ---------- | -------------------- | -------- | --------------------------------------------------- |
| `scope`    | string               | yes      | `view` for a local action, `state` for a shared one |
| `args`     | object               | no       | The arguments in order, each name with its type     |
| `let`      | object               | no       | Named values, in order                              |
| `guard`    | expression           | no       | The action applies only when it yields `true`       |
| `ops`      | list                 | yes      | The update operations, in order                     |
| `outcomes` | expression or object | no       | Every outcome a button may offer now                |
| `label`    | expression           | no       | The text of an action message                       |

- Every string value in an action, except `scope` and the argument types, is an expression,
  written without `{"=": …}`. A literal string is written as an expression, such as `'done'`.
- `args` lists at most 8 arguments, each name, which follows the action-name rule, with its type:
  `int`, a whole number from −9,007,199,254,740,991 to 9,007,199,254,740,991; `number`; `string`;
  or `bool`. Inside the action, `args` is a map from each name to its value.
- `let` holds at most 8 named values, evaluated once, in order, each reading the ones before it.
  Their names follow the action-name rule and are never a root name: `state`, `env`, `view`,
  `args`, `actor` or `roles`.
- A local action writes `view` and may read everything an action reads. A shared action writes
  `state`, never reads `view`, and reads only `env.now` of the environment, since the bot has no
  view, theme or direction; the load refuses one that does. Only shared actions declare
  `outcomes` or `label`.
- Applying an action checks its arguments, evaluates its named values and its guard, then runs its
  operations. The action is rejected, and nothing changes, when an argument is missing, extra or
  of the wrong type, the guard yields anything but `true`, an operation fails, the fuel runs out,
  or the new document passes its budget: 64 KB and nesting 64 for `state`, 4 KB for `view`.
- An action has its own fuel, 200,000 steps from its first named value to its last write. In
  actions, roles and outcomes, reading `state` or `view` whole costs a step per value it holds.

### Expressions

| Name           | Value                                     | Read in                                        |
| -------------- | ----------------------------------------- | ---------------------------------------------- |
| `view`         | The viewer's view                         | The tree, local actions                        |
| `args`         | The arguments of the action being applied | Actions                                        |
| `actor`        | `{id, name}` of who acts                  | Actions, roles                                 |
| `roles.<name>` | Whether the role holds                    | The tree for the viewer, actions for the actor |
| `env.viewer`   | `{id, name}` of the viewer, or `null`     | The tree, local actions                        |
| Named values   | The values of `let`                       | Their action, or their outcomes                |

- Map literals, `{key: value, …}`: each key a name or a string, used once, and kept in order. A
  map costs a fuel step per member.
- One comprehension, `[element for name in list if condition …]`: a `for`, then any number of
  `for` and `if` clauses, each binding or filtering for the ones after it, and `for name in
  [value]` binds a single value. A `for` over anything but a list makes the whole comprehension
  `null`, and an `if` keeps the bindings for which it yields `true`. Each clause holds the rest
  one nesting level deeper, a comprehension yields at most 1,000 items, and each item costs a fuel
  step. Inside a list, a `for` at the list's own level starts a comprehension.
- `index_of(list, value)` is the first position whose item equals `value`, as `==` compares, or
  −1, and `null` when `list` is not a list. `has(map, key)` is whether the map has a member `key`,
  and `null` unless given a map and a string.
- `roles.<name>` yields `true` only when the role's expression does. A role reads `state`,
  `actor` and `env.now`, never `view`, `args`, named values or another role. An unknown role name
  fails the load, and `roles` is read only as `roles.<name>`.
- A comprehension or a named value never binds `view`, `args`, `actor` or `roles`, and in a
  template that declares `actions` neither does a `repeat`, as no template binds `state` or `env`.
  In a template without `actions`, a `repeat` may still bind them, as in v0.

### Update operations

| Operation | Form                                            | Effect                                                   |
| --------- | ----------------------------------------------- | -------------------------------------------------------- |
| `set`     | `{"set": place, "to": value}`                   | Writes the value, adding a missing map member at the end |
| `add`     | `{"add": list, "value": value, "at": position}` | Inserts the value before `at`, or at the end without it  |
| `remove`  | `{"remove": place}`                             | Removes a list item or a map member                      |
| `move`    | `{"move": item, "into": list, "at": position}`  | Moves a list item into a list, before `at` or to its end |
| `inc`     | `{"inc": place, "by": number}`                  | Adds `by`, 1 when absent                                 |
| `toggle`  | `{"toggle": place}`                             | Negates a boolean                                        |

- Any operation may carry `when`, and then runs only when it yields `true`.
- A place is `state` in a shared action, or `view` in a local one, followed by `.name` and
  `[key]` steps. A string key steps into a map member and a whole number from 0 into a list item,
  so a negative or fractional number names nothing. The load refuses any other expression where a
  place belongs.
- `set` on the bare document replaces it, and the value must be a map; `remove` needs a step.
- Every expression of the action, the places' keys included, reads the documents as they stood
  before the action. The operations write a working copy in order, so after a `remove` from a
  list, a later write to a position in it lands on the item that shifted there.
- `move` counts `at` in the destination as it stood before the action, and adjusts it when the
  item moves within one list.
- An operation fails when its place is missing, other than the member `set` adds, or holds the
  wrong type: `add` and `move` need a list, `inc` a number and `toggle` a boolean. It also fails
  when `at` lies outside 0 to the list's length, `by` is not a number, or a sum is not finite.
- The working copy costs a step per value of the document, and `add`, `remove` and `move` cost a
  step per item they shift.

### Outcomes and the keyboard

- `outcomes` is an expression, or `{"let": {…}, "list": "…"}` with named values of its own, that
  yields a list of `{args, label}` maps: one button's arguments, as a map by name, and its text.
- Outcomes read `state`, `env.now` and their own named values. The load refuses outcomes that read
  `view`, `args`, `actor`, `roles` or the action's named values, since every member of the chat
  sees the same keyboard. The outcomes say what the state allows, and the guard says who may act.
- After every edit, the bot builds the message's keyboard from the new state: for each shared
  action with `outcomes`, in the template's order, a callback button per outcome, with the
  outcome's payload as its data and its label as its text. It skips an outcome whose arguments do
  not fit, whose payload passes 64 bytes or whose payload an earlier button carries, and stops at
  100 buttons, counting its own buttons first. An outcome past the cut has no button, so no
  trigger with its payload would run.
- A label is 1 to 64 characters. One that is empty or not a string becomes the payload, and a
  longer one is cut to 63 characters and `…`.
- Listing one keyboard's outcomes has its own 200,000 steps. The rows the buttons sit in are the
  bot's choice.

### Payloads

- A shared action travels as its payload: the action's name when it declares no arguments, else
  the name, `:` and its arguments as a compact JSON array in declared order, written as the
  resolved tree writes JSON: RFC 8785's numbers, and strings that escape only `"`, `\` and U+0000
  to U+001F, with JSON's two-character escapes where it has them and lowercase hex otherwise. So
  `unvote`, `vote:[1]` and `move:["wP5",4,4]`.
- The encoding is canonical, so the app and the bot write the same bytes. A reader splits the
  payload at its first `:`, parses the rest, checks it against the declaration, and refuses a
  payload that does not write back to the same bytes.
- A shared action with `outcomes` travels as callback data, the data of a button on the widget's
  own message, at most 64 bytes.
- A shared action without `outcomes` travels as an action message: a text message that replies to
  the widget message, holding the action's `label`, or its name without one, a line break, and a
  code block whose language is `tgw-action` and whose text is the payload. The whole message fits
  4,096 UTF-16 units, and `label` reads what the action's guard reads.
- Payloads name no actor: the bot takes it from the sender of the callback or the message.

### Triggers

| Property    | Value                     | Runs on                                             |
| ----------- | ------------------------- | --------------------------------------------------- |
| `on_tap`    | Triggers                  | A tap                                               |
| `draggable` | Triggers of local actions | The start of a drag, or a tap that picks            |
| `drop`      | Triggers                  | A drag released over the node, or a tap that places |
| `on_submit` | One trigger with `input`  | Submitting a `text_field`'s sheet                   |
| `link`      | A URL                     | A tap, which opens it                               |

- Every node but `repeat` and `connector` may carry `on_tap`, `draggable`, `drop` and `link`, and
  a `text_field` may carry `on_submit`. Shapes in `defs.symbols` carry none; their `use` does, as
  one target under the `use`'s key that covers every shape it draws. Invisible nodes have none.
- A trigger is `{"action": name, "args": {name: value}}`, with `args` left out for an action that
  takes none. Each value is a literal or `{"=": …}`, evaluated with the tree, so a trigger's
  arguments are fixed when the tree resolves, and may read `view`, a `repeat`'s item and
  everything else the tree reads.
- A trigger property holds one trigger or a list of up to 4, with at most one shared action. The
  triggers run in order, each reading the state and view as they stood before the gesture, and
  one that is rejected is skipped while the rest still run.
- A trigger would run when its action exists, its arguments fit, and its guard yields `true` for
  the viewer on the shown state and view. A shared one also needs the viewer set, fewer than 32
  actions pending, and a button with its payload or, without `outcomes`, action messages
  available. A list would run when its first trigger would, the rest riding along. Operations are
  not tried ahead, so a trigger that would run can still fail.
- A gesture would run on a node when its trigger list would. A target is disabled when none of its
  gestures would run.
- `link` keeps a URL that starts with `http://`, `https://` or `tg://`, in any case, within 2 KB;
  any other value drops it. The app opens it as it opens a `text_link` entity in a message, with
  the same confirmation.
- A node with `link` carries no other trigger, a node with `draggable` no `on_tap`, so every drag
  keeps its tap-to-pick alternative, and a `text_field` with `on_submit` no `on_tap`, `draggable`
  or `link`. A template that breaks one of these fails the load with `TWIDGET_ERROR_SCHEMA`, and
  so does a shared action in `draggable` or a trigger that names an unknown action or argument.
- A `text`'s entities that a message makes tappable, `url`, `text_link`, `mention`,
  `text_mention`, `hashtag`, `cashtag`, `email` and `phone_number`, act as they do in a message,
  through the app's own handling, and a tap inside one goes to it first. `bot_command` stays
  styling only, since a tap would send a message.

Tabs, a local action and its trigger on each tab of a `repeat`:

```json
"actions": {
  "select": {
    "scope": "view",
    "args": { "tab": "int" },
    "ops": [{ "set": "view.tab", "to": "args.tab" }]
  }
}
```

```json
"on_tap": { "action": "select", "args": { "tab": { "=": "index" } } },
"states": { "pressed": { "opacity": 0.6 } }
```

Gestures:

- Taps act on release, at the topmost target under the finger whose tap would run, which runs the
  first of its `on_tap`, `draggable` and `drop` that would run. A target lower in paint order is
  tried when the ones above it would run nothing, so a tap on an opponent's piece reaches the
  square beneath it. Tap-to-pick is two taps: one picks the source through its `draggable`, and
  one places it through a target's `drop`.
- A drag starts from the topmost target under the finger whose `draggable` would run, after a rest
  of about 150 ms. The crate runs those triggers on a scratch copy of the view and resolves from it
  for the rest of the drag, so the tree shows which drops would run. Releasing over a target whose
  `drop` would run runs it on the scratch view and keeps the result; releasing anywhere else, or a
  cancel, throws the scratch view away.
- The app draws an opaque, lifted copy of the source in an overlay above the chat list, so the
  copy can leave the bubble.
- Long press stays the message menu, and nothing runs while the chat selects messages. After a
  gesture that runs anything, the app resolves again.
- Hit-testing walks the boxes of flow nodes, frames and canvases and the bounds of shapes through
  their `transform`, shifts what a `scroll_x` has slid, and clips by every container as painting
  does, a strip's bleed included. It ignores state styles, transitions and keyframe animations.
- Tab and the arrow keys move keyboard focus through the targets in tree order, Enter and Space
  act as a tap, and focus leaves the widget at either end. A mouse shows the hand cursor over a
  target whose tap would run.

Chess, picked by drag or by tap. Each piece is a `use` in the pieces' `repeat`, bound as `p`, and
each square a `rect` in the squares' `repeat`, bound as `i`; `move` is a shared action:

```json
"actions": {
  "pick": {
    "scope": "view",
    "args": { "piece": "string" },
    "let": { "mine": "[p.id for p in state.pieces if p.side == state.turn]" },
    "guard": "roles.to_move && index_of(mine, args.piece) >= 0",
    "ops": [{ "set": "view.picked", "to": "args.piece" }]
  },
  "unpick": { "scope": "view", "ops": [{ "set": "view.picked", "to": "null" }] }
},
"roles": { "to_move": "state.players[state.turn] == actor.id" }
```

```json
"draggable": { "action": "pick", "args": { "piece": { "=": "p.id" } } }
```

```json
"drop": [
  {
    "action": "move",
    "args": {
      "piece": { "=": "view.picked" },
      "x": { "=": "i % 8" },
      "y": { "=": "floor(i / 8)" }
    }
  },
  { "action": "unpick" }
]
```

While nothing is picked, `view.picked` is `null`, which a `string` argument never takes, so no
square's drop would run. Once a piece is picked, the squares whose move has a button would.

Text input:

- A tap on a `text_field` with `on_submit` opens the app's native text sheet: the field's `label`
  as its title, its `placeholder` as hint, its `value` as the starting text, several lines when
  `lines` is above 1, and at most `max_length` characters, from 1 to 4,096 and 256 by default.
- `on_submit` is one trigger whose `input` names a `string` argument of its action that `args`
  leaves out. Submitting fills that argument with the text and runs the trigger, and cancelling
  runs nothing. A shared action there declares no `outcomes`, since text cannot be listed.

### State styles

- Every node but `repeat` and `connector` may carry `states`, `{"pressed": {…}, "hovered": {…},
  "focused": {…}, "disabled": {…}}`, each a map of properties the node may animate under
  *Animation*, with literal or `{"=": …}` values resolved with the tree. A literal property the
  node cannot animate fails the load with `TWIDGET_ERROR_SCHEMA`, and a value an expression
  produces that the property cannot take drops it from the style.
- A node's styles follow the nearest target at or above it, so a card's title can change color
  while the card is pressed. A node with no target at or above it ignores its `states`.
- `pressed` holds from the tap timeout until release or cancel, flashes briefly on a quicker tap,
  and holds on a drag's source for the whole drag. `hovered` holds while a mouse is over a target
  whose tap would run, or a dragged copy is over a target whose drop would run. `focused` follows
  keyboard focus, and `disabled` holds while no gesture of the target would run.
- States that hold together apply in the order hovered, focused, pressed, disabled, a later one
  winning per property.
- A state change eases with the node's transition, its own or the template's, and is immediate
  without one or under reduced motion. A running keyframe animation of the same property still
  wins. States never change layout.

### Identity

- The viewer is the account the app shows the chat to, as `{id, name}`: its Telegram user id, and
  its first name, a space and its last name when it has one. The app sets it on the instance once;
  the Widget Lab and other places without an account leave it `null`, and then no shared trigger
  would run.
- The tree reads the viewer as `env.viewer`, and roles for the viewer as `roles.<name>`. When the
  app applies an action, `actor` is the viewer; when the bot does, `actor` is the sender Telegram
  names, with the same members. A name the app knows differently is corrected by the bot's edit.

### Sync

- The instance holds the bot's state from the latest envelope and its `state_version`, the view,
  the pending shared actions and the message's buttons. The tree resolves from the shown state:
  the bot's state with each pending action applied again, in order.
- A shared action that applies is held as pending with its payload and the `env.now` it was sent
  at, and the crate hands the payload to the app, which sends it.
- A callback goes out as `messages.getBotCallbackAnswer` with the matching button's data. An error
  from the server, such as `DATA_INVALID` or a flood wait, drops the action at once. An answer
  without text leaves it pending, and an answer with text means the bot rejected it: the app shows
  the text as it shows any answer, and drops the action.
- An action message goes out as a reply to the widget message, and a failed send drops the action.
- With every edit, the bot raises `state_version` by one and lists in the envelope's `acks` each
  shared action it processed since its previous edit, applied or rejected, as `[user_id,
  payload]`, in the order it processed them, at most 100.
- On an edit, the app hands the new envelope to the instance:
  - Without `state_version`, it replaces the state as in v0 and drops every pending action.
  - With one no higher than the current one, it is stale and changes nothing.
  - With one higher by more than one, or without `acks`, it replaces the state and drops every
    pending action, since the app cannot tell what the bot applied.
  - Otherwise each ack that names the viewer drops the oldest pending action with its payload, and
    the remaining ones apply again on the new state, in order; one rejected there is dropped.
- A pending action that no ack names within 120 s of being sent is dropped. Pending actions live in
  memory only, and an edit keeps the view.
- The bot handles actions in the order they arrive. It decodes the payload, checks a callback's
  payload against the outcomes of its current state, and applies the action for the sender. It
  answers a callback at once, with no text when it applied the action and with the reason when it
  rejected it, so a bot never shows a notice on success. It edits the message with the new state,
  `state_version`, `acks` and keyboard, batching actions within Telegram's edit limits, and
  deletes action messages where it may.

### View persistence

- The app stores the view after every change, under the account, chat id and message id as the
  app knows them, with the template's id and version, and gives it back when it builds the
  instance again.
- A view stored under another template id or version is discarded, and so is one that is not a
  map within 4 KB; the instance then starts from the template's `view`. Every message, a
  forwarded copy included, has its own view.

### Where actions work

- Local actions work in every widget message.
- Callbacks work in a message that a bot sent, or that was sent through a bot in inline mode, and
  that is not a forward. The app passes that message's buttons to the instance; a forward, a
  message a person sent and a message without buttons pass none, so no callback trigger would run.
- Action messages work where the app can send a reply that the widget's bot reads: a private chat
  with that bot, and a group the bot is a member of. Inline-mode messages in chats without the
  bot, channels and forwards have none. The app tells the instance whether they are available.

### Fallback

- A v0 client shows the widget as before, and the bot's keyboard under it as plain buttons with
  the outcomes' labels.
- A v1 app hides each callback button whose data decodes to a shared action of the widget's
  template, and keeps the bot's other buttons under the widget. It also hides each message that
  replies to a widget message and whose first `tgw-action` block decodes to one of that widget's
  actions.

### Accessibility

- Every target is one screen-reader element, in tree order: a button for taps, drags and drops, a
  link for `link`, and an edit field for a `text_field` with `on_submit`.
- Its label is the node's `a11y`, else the strings it and its descendants draw, as *Layout
  direction* lists them, joined by spaces, else its first trigger's action name. Canvas targets
  should set `a11y`.
- A target whose tap would not run is announced as disabled. A target whose only gesture is `drop`
  reaches screen readers and keyboard focus only while its drop would run.
- Activation acts as a tap, and drags go through tap-to-pick, so a screen reader moves a piece with
  two activations. A template puts state in the label, such as
  `{"=": "view.picked == p.id ? 'White pawn, picked' : 'White pawn'"}`.

### Resolved tree and C API

- A top-level `interaction` member follows `motion`: `null` when the tree has no target, else the
  targets and the state styles, each keyed by node, as `docs/RESOLVED.md` details. The format
  stays 1.
- ABI 1 grows by these functions, and `TWidgetEnv` is unchanged:

| Function                       | Purpose                                                                  |
| ------------------------------ | ------------------------------------------------------------------------ |
| `twidget_instance_set_viewer`  | Sets the viewer's id and name, once per instance                         |
| `twidget_instance_set_buttons` | Passes the message's callback data and whether action messages work      |
| `twidget_instance_apply_edit`  | Applies an edited envelope: its state, version and acks                  |
| `twidget_dispatch`             | Runs a gesture on a target: tap, drag start, drop, drag cancel or submit |
| `twidget_dispatch_result`      | Writes the last dispatch's result: what ran and what to send             |
| `twidget_pending_drop`         | Drops a pending action after a failed send or a rejecting answer         |
| `twidget_instance_view`        | Writes the view for the app to store                                     |
| `twidget_instance_set_view`    | Restores a stored view                                                   |

- `twidget_instance_set_state` stays, and acts as an edit without `acks`.
- A rejected action is a result, not a failure: a dispatch returns `TWIDGET_OK` and its result
  says what ran.
- The Rust API gives the bot the same reducers: the keyboard of a state, and applying a payload
  for an actor.

### Budgets

| Budget                                | Initial limit                         |
| ------------------------------------- | ------------------------------------- |
| Actions per template                  | 64                                    |
| Roles per template                    | 16                                    |
| Arguments per action                  | 8                                     |
| Named values per action or outcomes   | 8                                     |
| Operations per action                 | 32                                    |
| Triggers per gesture                  | 4                                     |
| Fuel per action                       | 200,000 steps                         |
| Fuel per keyboard                     | 200,000 steps                         |
| Interaction fuel per resolve          | 200,000 steps                         |
| Interaction targets per resolved tree | 1,000                                 |
| Pending actions per instance          | 32                                    |
| Pending timeout                       | 120 s                                 |
| `acks` per envelope                   | 100                                   |
| View                                  | 4 KB                                  |
| Callback data                         | 1 to 64 bytes                         |
| Buttons per message                   | 100                                   |
| Outcome label                         | 1 to 64 characters                    |
| Action message                        | 4,096 UTF-16 units                    |
| `link`                                | 2 KB                                  |
| `max_length`                          | 1 to 4,096 characters, 256 by default |

The interaction fuel pays for the guards and checks that decide what would run. A literal out of
range fails the load with `TWIDGET_ERROR_BUDGET`. At dispatch, an action that passes a budget is
rejected and changes nothing. More than 1,000 targets, or running out of interaction fuel, fails
the resolve with `TWIDGET_ERROR_BUDGET`, and an envelope with more than 100 `acks` fails with it
too, leaving the instance as it was.

### Action sequences

Conformance gains a fifth layer, action sequences:

- A sequence, `fixtures/sequences/<name>.json`, is `{"tgw_sequence": 1, "envelope": …, "viewer":
  …, "action_messages": …, "steps": […]}`: an envelope from `fixtures/envelopes/`, the viewer,
  whether action messages are available, and steps run in order. The reference bot builds the
  first keyboard from the envelope's state.
- Steps are `{"tap": key}`; `{"drag": key}` and `{"drop": key}`, with `{"drop": null}` for a
  cancel; `{"submit": key, "text": …}`; `{"answer": text}` and `{"fail": true}`, for the oldest
  pending action; `{"bot": [entry, …]}`, where the reference bot processes the oldest payload the
  app sent, `"sent"`, or another member's, `{"actor": …, "payload": …}`; `{"edit": "seen"}` and
  `{"edit": "lost"}`, where the bot edits with everything processed since its last edit and the
  app receives the edit or never does; `{"envelope": …}`, a hand-written edit; and
  `{"wait": ms}`, which moves the clock.
- `twidget-cli sequence <file>` writes `{"tgw_sequence_result": 1, "steps": […]}`: for each step,
  the dispatch result or `null`, the pending payloads, the view, the shown state and the
  `interaction` member. The results are checked in under `fixtures/expected/sequences/`, and the
  environment is the fixed test one.
- The reference bot is the crate's own: the outcomes, guards and operations `twidget-bot` runs.

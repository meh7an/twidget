# Resolved tree, format 1

`twidget_resolve` writes the tree an app lays out and paints: every expression evaluated, every
default written out, and every `repeat`, `use` and `connector` expanded. Apps never evaluate an
expression, apply a default, or see a node type beyond the thirteen below.

## Document

```json
{"tgw_resolved": 1, "width": "fill", "root": {...}, "motion": null, "interaction": null}
```

| Field          | Value                                                                             |
| -------------- | --------------------------------------------------------------------------------- |
| `tgw_resolved` | `1`, the format number `TWIDGET_RESOLVED_FORMAT` names                            |
| `width`        | `"fill"` for the bubble's content width, or a preferred width in units            |
| `root`         | The root node, or `null` when it resolves to nothing, as an invisible root does   |
| `motion`       | `null` when nothing moves, or the animations and transitions under *Motion*       |
| `interaction`  | `null` when nothing is interactive, or the targets and styles under *Interaction* |

The JSON is compact UTF-8 with members in the order this document gives and numbers in their
shortest round-trip form. Identical bytes mean an unchanged tree, so the app can skip layout.

## Nodes

A node is an object with `type`, then `key`, then every property of its type in the order of its
table below, then `children` for `column`, `row`, `scroll_x` and `canvas`, or `child` for
`frame`. Every property is always present, and `null` means what its table says. Only visible
nodes are written, so nodes carry no `visible`.

### Keys

A key is the node's path from the root, stable from one resolve to the next, so an app can match
nodes across resolves to animate them.

- The root is `0`, and each other node adds `/` and its index among its parent's children in the
  template. A node whose template sets `key` uses that key in place of its index, and so does the
  root in place of `0`.
- The items of a `repeat` add `:` and the item's key to the `repeat`'s own segment: the key the
  `repeat` gives the item, else the key its child sets, else the item's index. A chess piece is
  `0/1/2:wK`, and nested `repeat`s compose, as in `0/0:To do/1:c1`.
- Keys from the template have `/`, `:` and `#` replaced by `_`, and a number key is written as
  the template's `string()` writes it. When a segment repeats among one parent's children, the
  later ones get `#2`, `#3` and so on, so keys are unique.
- The shapes a template's `use` draws take the `use`'s key plus `/` and each shape's own key or
  its index in the symbol, as in `0/1/2:wK/0`. A template's `connector` becomes paths: its line
  keyed `/0` under the connector's key, its arrowhead at `to` keyed `/1` and at `from` `/2`.

### Containers

| Type       | Properties                                                                                |
| ---------- | ----------------------------------------------------------------------------------------- |
| `column`   | `a11y`, `gap`, `padding`, `align`, `background`, `radius`, `border`, `width`, `weight`    |
| `row`      | The same as `column`                                                                      |
| `scroll_x` | `a11y`, `gap`, `padding`, `width`, `weight`                                               |
| `canvas`   | `a11y`, `view_box`, `background`, `radius`, `width`, `weight`                             |
| `frame`    | `a11y`, `x`, `y`, `w`, `h`, `padding`, `align`, `background`, `radius`, `border`          |

A `frame`'s `child` is its one flow node, or `null` when it has none. Every child of a
`scroll_x` has a number `width`.

### Flow leaves

| Type         | Properties                                                                                          |
| ------------ | --------------------------------------------------------------------------------------------------- |
| `text`       | `a11y`, `text`, `entities`, `style`, `color`, `align`, `max_lines`, `tabular`, `width`, `weight`    |
| `text_field` | `a11y`, `label`, `value`, `placeholder`, `lines`, `width`, `weight`                                 |
| `chart`      | `a11y`, `kind`, `labels`, `series`, `y_min`, `y_max`, `height`, `legend`, `width`, `weight`         |
| `divider`    | `a11y`, `color`, `width`, `weight`                                                                  |

### Canvas shapes

| Type      | Properties                                                                                                   |
| --------- | ------------------------------------------------------------------------------------------------------------ |
| `rect`    | `a11y`, `x`, `y`, `w`, `h`, `rx`, `fill`, `stroke`, `stroke_width`, `opacity`, `dash`, `cap`, `join`         |
| `ellipse` | `a11y`, `cx`, `cy`, `rx`, `ry`, `fill`, `stroke`, `stroke_width`, `opacity`, `dash`, `cap`, `join`           |
| `line`    | `a11y`, `x1`, `y1`, `x2`, `y2`, `fill`, `stroke`, `stroke_width`, `opacity`, `dash`, `cap`, `join`           |
| `path`    | `a11y`, `d`, `fill`, `stroke`, `stroke_width`, `opacity`, `dash`, `cap`, `join`                              |

Coordinates and sizes of shapes are in view-box units. Shapes drawn from a symbol also carry
`transform`, `[a, b, c, d, e, f]` in SVG matrix order, after `join`: it maps the shape's own
coordinates, stroke width and dash included, into the canvas, as SVG's `transform` attribute
does. `d` is never empty, since a path without data is left out, and uses only absolute `M`,
`L`, `C` and `Z`. Every subpath starts with an `M`: drawing that goes on after a `Z` begins with
an `M` back to the closed subpath's start, so no app has to know where its path library puts the
pen after a close.

A connector arrives as a `path` for its line, stopping at each arrowhead's base, and a closed
`path` for each arrowhead, filled in the line's stroke color. The line carries the connector's
`a11y`, and the first shape a `use` draws carries the `use`'s unless it has its own.

## Values

| Property                                   | Value                                                                                   |
| ------------------------------------------ | --------------------------------------------------------------------------------------- |
| `a11y`                                     | The accessibility label, or `null` for none                                             |
| `gap`, `radius`, `rx`, `ry`, sizes         | Numbers in units, or in view-box units inside a canvas                                  |
| `padding`                                  | `[top, end, bottom, start]` in units; `end` and `start` follow the layout direction     |
| `align`                                    | `start`, `center`, `end` or, on boxes, `stretch`                                        |
| `background`, `fill`, `stroke`             | A color, or `null` to paint nothing                                                     |
| `color`                                    | A color                                                                                 |
| `border`                                   | `{"color": …, "width": …}` with both members, or `null` for none                        |
| `width`                                    | A width in units, or `null` when the node sets none                                     |
| `weight`                                   | A row child's share of the width left after fixed widths                                |
| `view_box`                                 | `[w, h]`, each from 1 to 10,000                                                         |
| `text`, `label`, `value`, `placeholder`    | Plain text                                                                              |
| `entities`                                 | Bot API `MessageEntity` objects, each with every member it was given                    |
| `style`                                    | `body`, `caption`, `title`, `display` or `mono`                                         |
| `max_lines`                                | A line count, or `null` for no limit                                                    |
| `tabular`                                  | `true` for fixed-width digits                                                           |
| `lines`                                    | The lines of space a `text_field` shows                                                 |
| `kind`                                     | `line` or `bar`                                                                         |
| `labels`                                   | Strings, one per x position                                                             |
| `series`                                   | Objects with `name`, `values` and `color`; each value is a number or `null` for a gap   |
| `y_min`, `y_max`                           | The bottom and top of the y axis                                                        |
| `height`                                   | A height in units, or `null` for 0.6 times the chart's width                            |
| `legend`                                   | Whether to show the legend                                                              |
| `stroke_width`, `opacity`                  | Numbers; opacity runs from 0 to 1                                                       |
| `dash`                                     | `[on, off]` in view-box units, or `null` for a solid stroke                             |
| `cap`, `join`                              | `butt`, `round` or `square`; `miter`, `round` or `bevel`                                |

Colors are theme tokens, such as `accent`, a token with an alpha, such as `accent@0.4`, or fixed
`#RRGGBB` and `#AARRGGBB` values. Apps map tokens to their own palettes, so a theme change needs
a repaint and never a resolve.

Entity offsets and lengths are whole numbers of UTF-16 code units, and every entity lies inside
its text: the crate clips one that runs past the end and leaves out one that starts there.

## Motion

`motion` is `null` when the template sets no transition and no node animates. Otherwise it holds
the keyframe animations and transitions of the spec's *Animation* clarification, every default
written out, so an app interpolates without reading the template.

```json
"motion": {"transition": null, "transitions": [], "animations": [{"key": "0/1", ...}]}
```

| Field         | Value                                                                                  |
| ------------- | -------------------------------------------------------------------------------------- |
| `transition`  | The template's default transition, or `null` when it sets none                         |
| `transitions` | `{"key": …, "transition": …}` for each node that sets its own, `null` meaning `none`   |
| `animations`  | The animation entries, each keyed by its node                                          |

`transitions` and `animations` follow the tree's order, a node before its children, and a node's
entries keep the template's order. Keys are the node keys of the tree, so a `repeat` gives each
item its own entries, a `use`'s entries repeat for each shape it draws, ahead of the shape's own,
and an invisible node has none. When a key has more than one entry for a property, the last one
applies.

### Transitions

| Member     | Value                                                                    |
| ---------- | ------------------------------------------------------------------------ |
| `duration` | Milliseconds, from 1 to 60,000                                           |
| `easing`   | `"linear"` or a cubic Bézier `[x1, y1, x2, y2]`                          |
| `enter`    | `"fade"` or `"none"`, for nodes the new tree adds                        |
| `exit`     | `"fade"` or `"none"`, for nodes the new tree removes                     |
| `text`     | `"crossfade"`, `"roll_up"`, `"roll_down"`, `"count"` or `"none"`         |

Between two trees a shape's `transform` transitions too, member by member, so a symbol that a
`use` moves or resizes glides.

### Animations

| Member      | Value                                                                                      |
| ----------- | ------------------------------------------------------------------------------------------ |
| `key`       | The animated node's key                                                                    |
| `property`  | The property it animates                                                                   |
| `keyframes` | 2 to 32 `{"at": …, "value": …, "easing": …}`, `at` rising from 0 to 1                      |
| `duration`  | Milliseconds, from 1 to 60,000                                                             |
| `delay`     | Milliseconds, from 0 to 60,000                                                             |
| `repeat`    | `"loop"`, `"alternate"`, or a count from 1 to 1,000, `once` being 1                        |
| `origin`    | Fractions `[fx, fy]` of a flow node's or frame's box, or a shape's view-box point `[x, y]` |

A keyframe's `easing` is `"linear"`, `"hold"` or a cubic Bézier `[x1, y1, x2, y2]`; the named
curves arrive as their Bézier arrays, and the last keyframe's easing is written but never used.
Every `value` is written, the crate filling a missing one with the node's own value.

| Property                                | Value                                                   |
| --------------------------------------- | ------------------------------------------------------- |
| `opacity`                               | A number from 0 to 1                                    |
| `translate`                             | `[x, y]` in units, or in view-box units inside a canvas |
| `scale`                                 | A number                                                |
| `rotate`                                | Degrees, clockwise                                      |
| `background`, `color`, `fill`, `stroke` | A color, or `null` for `none`                           |
| Shape geometry and `stroke_width`       | A number in view-box units                              |
| `dash`                                  | `[on, off]` in view-box units                           |
| `count`                                 | A whole number                                          |

A `dash` entry needs a pair at every keyframe, so the crate drops one whose node has no dash and
whose keyframes leave a value out.

## Interaction

`interaction` is `null` when the tree has no target. Otherwise it says what each target does now
and how nodes look while it is used, as the spec's *Interactivity (v1.0)* describes, so an app
hit-tests, runs gestures and paints states without reading the template.

```json
"interaction": {"targets": [{"key": "0/1/3:wP5", "role": "button", ...}], "styles": []}
```

| Field     | Value                                                                  |
| --------- | ---------------------------------------------------------------------- |
| `targets` | Each target, in tree order, a node before its children                 |
| `styles`  | Each node with state styles, in tree order, with the target it follows |

### Targets

| Member     | Value                                                                  |
| ---------- | ---------------------------------------------------------------------- |
| `key`      | A node's key, or a `use`'s, covering every shape it draws              |
| `role`     | `button`, `link` or `text_field`, as a screen reader announces it      |
| `label`    | The screen-reader label                                                |
| `disabled` | `true` when no gesture would run                                       |
| `tap`      | Whether a tap would run anything                                       |
| `drag`     | Whether a drag would start, or `null` when the node is not `draggable` |
| `drop`     | Whether a drop would run, or `null` when the node has no `drop`        |
| `input`    | `{"max_length": …}` for a `text_field` with `on_submit`, else `null`   |
| `link`     | The URL a tap opens, or `null`                                         |

- A key that names no node is a `use`'s: its target covers the shapes whose keys start with it and
  a `/`.
- A tap goes to the topmost target under the point whose `tap` is `true`, a drag starts from the
  topmost whose `drag` is `true`, and a drop goes to the topmost whose `drop` is `true`, hit-tested
  over resolved geometry with strips' slides and containers' clips. A tap on a `link` opens it,
  and a tap on an `input` opens the text sheet.
- During a drag the tree resolves from the drag's scratch view, so `drop` says which targets
  accept the dragged source.

Accessibility:

- Each target is one screen-reader element, in the order `targets` lists them, with its `role`
  and `label`. One whose `tap` is `false` is announced as disabled, and one whose only gesture is
  `drop` is exposed only while `drop` is `true`.
- Keyboard focus moves through the same elements in the same order, Enter and Space act as a tap,
  and activating an element acts as a tap.

### Styles

| Member     | Value                                            |
| ---------- | ------------------------------------------------ |
| `key`      | The styled node's key                            |
| `target`   | The key of the target whose state it follows     |
| `pressed`  | The properties it takes while pressed, or `null` |
| `hovered`  | The same while hovered                           |
| `focused`  | The same while focused                           |
| `disabled` | The same while disabled                          |

Each style maps property names to values written as *Motion*'s animation values are, and a
`use`'s styles appear once for each shape it draws. States that hold together apply in the order
`hovered`, `focused`, `pressed`, `disabled`, a later one winning per property, and a change eases
with the node's transition from `motion`.

### Dispatch results

`twidget_dispatch_result` writes the last dispatch's result:

```json
{"tgw_dispatch": 1, "ran": [{"action": "move", "result": "applied"}, {"action": "unpick", "result": "applied"}], "send": {"pending": 1, "via": "callback", "data": "move:[\"wP5\",4,4]", "label": null, "payload": null}}
```

| Field          | Value                                                             |
| -------------- | ----------------------------------------------------------------- |
| `tgw_dispatch` | `1`                                                               |
| `ran`          | Each trigger of the gesture, in order, with its action and result |
| `send`         | `null`, or what the app sends for the shared action that applied  |

A result is `applied`; `arguments`, when they did not fit; `guard`; `unavailable`, when no button
matches, action messages are off, there is no viewer or 32 actions are pending; or `failed`, when
an operation failed or a budget ran out.

| Member    | Value                                                    |
| --------- | -------------------------------------------------------- |
| `pending` | The number to pass `twidget_pending_drop`                |
| `via`     | `callback` or `message`                                  |
| `data`    | The button data to send, or `null` for a message         |
| `label`   | The text before the code block, or `null` for a callback |
| `payload` | The code block's text, or `null` for a callback          |

### Buttons

`twidget_instance_set_buttons` reads a JSON array of strings: the callback data of the message's
buttons, in keyboard order. Buttons of other kinds, and data that is not UTF-8, are left out, and
a message without buttons passes `[]`.

# Resolved tree, format 1

`twidget_resolve` writes the tree an app lays out and paints: every expression evaluated, every
default written out, and every `repeat`, `use` and `connector` expanded. Apps never evaluate an
expression, apply a default, or see a node type beyond the thirteen below.

## Document

```json
{"tgw_resolved": 1, "width": "fill", "root": {"type": "column", "key": "0", ...}, "motion": null}
```

| Field          | Value                                                                           |
| -------------- | ------------------------------------------------------------------------------- |
| `tgw_resolved` | `1`, the format number `TWIDGET_RESOLVED_FORMAT` names                          |
| `width`        | `"fill"` for the bubble's content width, or a preferred width in units          |
| `root`         | The root node, or `null` when it resolves to nothing, as an invisible root does |
| `motion`       | `null` when nothing moves, or the animations and transitions under *Motion*     |

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
item its own entries, a `use`'s entries repeat for each shape it draws, and an invisible node has
none.

### Transitions

| Member     | Value                                                                    |
| ---------- | ------------------------------------------------------------------------ |
| `duration` | Milliseconds, from 1 to 60,000                                           |
| `easing`   | `"linear"` or a cubic Bézier `[x1, y1, x2, y2]`                          |
| `enter`    | `"fade"` or `"none"`, for nodes the new tree adds                        |
| `exit`     | `"fade"` or `"none"`, for nodes the new tree removes                     |
| `text`     | `"crossfade"`, `"roll_up"`, `"roll_down"`, `"count"` or `"none"`         |

### Animations

| Member      | Value                                                                                   |
| ----------- | --------------------------------------------------------------------------------------- |
| `key`       | The animated node's key                                                                 |
| `property`  | The property it animates                                                                |
| `keyframes` | 2 to 32 `{"at": …, "value": …, "easing": …}`, `at` rising from 0 to 1                   |
| `duration`  | Milliseconds, from 1 to 60,000                                                          |
| `delay`     | Milliseconds, from 0 to 60,000                                                          |
| `repeat`    | `"loop"`, `"alternate"`, or a count from 1 to 1,000, `once` being 1                     |
| `origin`    | `[fx, fy]` as fractions of a flow node's box, or a view-box point `[x, y]` for a shape  |

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

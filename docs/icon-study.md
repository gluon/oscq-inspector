# The application icon: what it is now, and three directions

Written on 2026-11-19 on branch `void/presets-icon`, from `c6bb5b7` (0.4.2), answering one
report: the icon is to be redone. Nothing that ships was changed. `icon::rgba` writes exactly the
bytes it wrote before, and the packaging set it produces is byte identical, checked by rendering
it before and after the study was added:

```
cargo run --release -p oscq-inspector --example icons -- OUT_DIR
find . -name "*.png" | sort | xargs shasum | shasum
e025f03573f10d13f4f49fbb49c1418db5047cd7   before and after
```

## What the icon is now

An address space tree on a rounded square: a root, a trunk, two branches, the lower leaf drawn as
a live value. Drawn in code, `crates/oscq-inspector/src/icon.rs`, as signed distances, so one
function is the window icon, the macOS `.icns`, the Windows resource and the Linux PNG.

Two things about it are worth stating before any drawing is judged, because they are facts and
not taste:

- **Its colours belong to no palette of the application.** The body is `#1F3A5F`, a dark blue
  that is in neither theme of `style.rs`. The live node is `#FFB020`, which is not the one signal
  colour `#FF7A1A` that everything live in the window is drawn in. The design is a greyscale
  ground with one signal colour, and the icon is outside it.
- **Its strokes are the thinnest of any drawing in the application**, 0.045 of the body, with
  five separate elements in the square. That is what has to survive 32 pixels and 16.

## The three directions

Sober, massive, brutalist, by the human's decision of 2026-11-19, which set aside a first three:
flat blocks with square ends, no circle, no taper, no gradient, nothing but rectangles, and the
signal colour on one block and no more. The body keeps the rounded square of the macOS grid,
which is the platform's shape and not the drawing's.

| Name | What it is | Files |
| --- | --- | --- |
| `bars` | Three slabs left aligned, each shorter than the one above, on a near black body: an address space read as levels. The shortest is the live one | `bars-1024.png`, `bars-256.png`, `bars-32.png` |
| `stem` | A stem and two arms, all square, black on white: the tree with every curve taken out of it, and the only one of the three drawn the other way round | `stem-1024.png`, `stem-256.png`, `stem-32.png` |
| `quad` | Four blocks in a square on a near black body, one of them live: the grid of the window's own preset slots, and the simplest shape of the three at 16 pixels | `quad-1024.png`, `quad-256.png`, `quad-32.png` |

The icon that ships is rendered beside them under `current-*`, so the four are judged together.

`bars` and `quad` are a near black body, `#0A0A0A`, with a `#333333` edge so the shape keeps an
outline on a dark Dock. `stem` is the light theme's white with its `#A8A8A8` border, so that the
choice is not three black squares. All three take `#FF7A1A`, the one signal colour, on one block.

### The first three, set aside

A first set, drawn the same day and rejected by the human: `tree`, the shipped drawing held to
the palette; `path`, the slash of an OSC address with a live node on it; `signal`, a node and the
arcs of what it publishes. They kept the drawing style of the icon they were meant to replace.
They are not rendered any more and their code is gone; the commit that added them is where they
live.

## The sheets

`docs/images/icon-study/dock-dark.png` and `dock-light.png`: the four icons in rows, in the order
**current, bars, stem, quad**, at 256, 128, 64 and 32 pixels across. Nothing is scaled: a 32
pixel icon on these sheets is 32 pixels, so what the Dock and the window title really draw is
what is shown. The grounds are a dark desktop's Dock, `#262626`, and a light one's, `#E8E8E8`.

Each master is also drawn alone on each ground, `NAME-1024-dark.png` and `NAME-1024-light.png`.

Everything is written by one command:

```
cargo run -p oscq-inspector --features screenshot --example icon_study -- OUT_DIR
```

## What the drawings are held to

`icon::tests::every_direction_draws_its_three_colours_inside_the_body` checks, for each direction
and at 32, 256 and 1024 pixels, that the ground, the blocks and the live block are all drawn,
that the corners stay transparent, and that no pixel of the live block sits on the body's outline.

That last clause is there because a drawing failed it. The `signal` direction of the first set
struck its arcs at 0.56 of the body over 65 degrees, which put the outermost one on the rounded
square's edge: the corners cut it and the live arc bled off the shape at 1024 and at 256. The
test was watched failing on that geometry:

```
test icon::tests::every_direction_draws_its_three_colours_inside_the_body ... FAILED
Signal: a live pixel at 484,51 is on the body's outline
```

and passing once the radius was brought in. It is run with `cargo test -p oscq-inspector
--features screenshot --bin oscq-inspector icon::`.

## What this study does not establish

- No direction was seen in a real Dock, in the Finder, in a window's title bar or under
  Notification Centre. The sheets are the Dock's grounds and the true pixel sizes, drawn on this
  machine; they are not the Dock. macOS also masks an icon to its own squircle and adds its own
  shadow, neither of which these sheets apply.
- No direction was rendered as an `.icns` or a Windows resource, and none was installed. Whether
  macOS's own rounding and shadow sit well on this body was not checked.
- Nothing here says which reads best to anyone but its author. The sizes and the grounds are
  given so the choice can be made on what the screen does, not on a description.

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

All three are held to the palette: a near black ground `#0A0A0A` with a `#333333` edge so the
shape keeps an outline on a dark Dock, white ink, and the signal colour `#FF7A1A` for the one live
mark. All three are drawn heavier than the icon that ships, 0.065 of the body.

| Name | What it says | Files |
| --- | --- | --- |
| `tree` | What the icon says today, in the palette and heavier: a root, a trunk, two branches, the lower leaf live | `tree-1024.png`, `tree-256.png`, `tree-32.png` |
| `path` | The slash of an OSC address, a node where it starts and a live node where it ends. The fewest elements of the three | `path-1024.png`, `path-256.png`, `path-32.png` |
| `signal` | A node and the arcs of what it publishes, the outermost live: discovery rather than address space | `signal-1024.png`, `signal-256.png`, `signal-32.png` |

The icon that ships is rendered beside them under `current-*` so the four are judged together.

## The sheets

`docs/images/icon-study/dock-dark.png` and `dock-light.png`: the four icons in rows, in the order
**current, tree, path, signal**, at 256, 128, 64 and 32 pixels across. Nothing is scaled: a 32
pixel icon on these sheets is 32 pixels, so what the Dock and the window title really draw is
what is shown. The grounds are a dark desktop's Dock, `#262626`, and a light one's, `#E8E8E8`.

Each master is also drawn alone on each ground, `NAME-1024-dark.png` and `NAME-1024-light.png`.

Everything is written by one command:

```
cargo run -p oscq-inspector --features screenshot --example icon_study -- OUT_DIR
```

## What the drawings are held to

`icon::tests::every_direction_draws_its_three_colours_inside_the_body` checks, for each direction
and at 32, 256 and 1024 pixels, that the ground, the ink and the live mark are all drawn, that
the corners stay transparent, and that no pixel of the live mark sits on the body's outline.

That last clause is there because the first drawing of `signal` failed it. Its arcs were struck
at 0.56 of the body over 65 degrees, which put the outermost one on the rounded square's edge: the
corners cut it and the live arc bled off the shape at 1024 and at 256. The test was watched
failing on that geometry before the radius was brought in to 0.46 over 55 degrees:

```
test icon::tests::every_direction_draws_its_three_colours_inside_the_body ... FAILED
Signal: a live pixel at 484,51 is on the body's outline
```

and passing after. It is run with `cargo test -p oscq-inspector --features screenshot --bin
oscq-inspector icon::`.

## What this study does not establish

- No direction was seen in a real Dock, in the Finder, in a window's title bar or under
  Notification Centre. The sheets are the Dock's grounds and the true pixel sizes, drawn on this
  machine; they are not the Dock.
- No direction was rendered as an `.icns` or a Windows resource, and none was installed. Whether
  macOS's own rounding and shadow sit well on this body was not checked.
- Nothing here says which reads best to anyone but its author. The sizes and the grounds are
  given so the choice can be made on what the screen does, not on a description.

# The preset section, audited from the window, and three designs for its capture

Written on 2026-11-19 on branch `void/presets-icon`, from `c6bb5b7` (0.4.2). It answers one
report: the capture part of the preset section is not clear. It is an audit and a proposal. No
shipped behaviour was changed, and the designs of the last section are drawings, not working
controls.

## How every image here was taken

On the human's Mac, against two synthetic servers of this repository and nothing else:

```
cargo run --release -p oscq-core --example synthetic_server -- --port 5678 --nodes 12 --name Alpha
cargo run --release -p oscq-core --example synthetic_server -- --port 5679 --nodes 4  --name Beta
```

Alpha publishes twelve methods, `/g0/p0` to `/g0/p11`, each `f`, `ACCESS` 3, `VALUE` 0.5. Beta
publishes the first four of them, `/g0/p0` to `/g0/p3`, the same type. Both are synthetic servers
of the test suite, not an application: no real server was reached for any measurement below, and
nothing here says how Max, VDMX or OSCQuery Helper behave.

Each image is one launch of the release binary, with a config file in a scratch `HOME` so the
human's own settings were never touched, and the window writes the frame itself:

```
HOME=... OSCQ_INSPECTOR_STARTUP=connect-all,tree \
  OSCQ_INSPECTOR_SHOT_DO=capture:3 OSCQ_INSPECTOR_SHOT=out.png ./oscq-inspector
```

`OSCQ_INSPECTOR_SHOT` and `OSCQ_INSPECTOR_SHOT_DO` need the `screenshot` feature, which no
release build takes. `SHOT_DO` performs one word per frame, as a hand clicks once per frame, and
the words go through the same `act` a click goes through, so the window reaches the state a click
leaves it in. Why the application draws its own frame rather than the system capturing its
window is in `docs/decisions.md` under 2026-11-19, and what it avoids is in
`docs/images/screenshots/README.md`.

The window is 1280 by 800 points and the frame comes back at 2x, 2560 by 1600 pixels, without a
title bar. The full window images are cut to 2124 by 1600: the server pane is left out of them,
since it listed a server of the human's own network that has nothing to do with this audit, and
nothing of the preset section lives in that pane. The close images of the section are cuts of the
same frames.

## What a slot actually holds

Captured into slot 3 on Alpha with nothing pinned, then read out of the config the window wrote:

```
slot 3  server 127.0.0.1:5678  name Alpha  values 12
  {'path': '/g0/p0',  'type_tags': 'f', 'args': [{'Float': 0.5}]}
  {'path': '/g0/p1',  'type_tags': 'f', 'args': [{'Float': 0.5}]}
  {'path': '/g0/p10', 'type_tags': 'f', 'args': [{'Float': 0.5}]}
```

A slot holds the server's `host:port`, the name it declared, and one entry per value: the path,
the type tags the method had when captured, and the arguments. That is all of it. There is no
time of capture, no count of what was left out, and no mark on a value that was sent and never
confirmed.

## Which nodes a capture takes

| Case | Taken | Proof |
| --- | --- | --- |
| Nothing pinned | Every writable method of the loaded tree: 12 of Alpha's 12 | the config above |
| Three nodes pinned | Only those three, `/g0/p0`, `/g0/p1`, `/g0/p2` | `08-capture-scope-pinned.png` and the config |
| A method with no value, or whose type cannot be kept | Left out and counted | `presets::tests::capture_keeps_writable_values_and_counts_the_rest` |
| A method written but not yet confirmed | The value sent, not the value read | the same test, "the value sent, not yet read" |

Two things the README does not say and the PRD does. The scope is the **loaded** tree, not the
server's address space: a subtree the window has not fetched is not captured, and nothing says
so. And the value taken may be one the server never acknowledged, which is a value the window
itself draws as unconfirmed everywhere else (law 1).

## What Recall does

On the same server, Alpha: twelve messages, sent at once as one block, shown in the Traffic tab
as `out preset 03 (12 messages)`, and Alpha's own UDP receiver counted `OSC packets received 12`.
See `05-recall-same-server.png`.

On another server, Beta, from a slot captured on Alpha: only a writable method at the same path
with the same type tags is sent. Beta has four of Alpha's twelve paths, so four were sent and
eight matched nothing; Beta's receiver counted `OSC packets received 4`. See
`06-recall-other-server.png`.

## What is overwritten, and when

A capture replaces the slot whole. Slot 3 held twelve values from Alpha; a capture on Beta into
the same slot left four values from Beta, and the twelve are gone. There is no confirmation, no
trace of what was there, and no undo. See `07-overwrite-no-confirmation.png`.

A plain click captures into an empty slot and recalls a full one; a shift click, or the right
click menu, captures into a full one. Nothing in the code removes a preset, so **a slot that
holds values can never be emptied**, only replaced.

## The points the screen does not make readable

Each line names the image that shows it. Where the finding is the absence of something, the image
is the section at rest and the absence is what it shows.

1. **The word "Capture" is nowhere on screen.** The section is headed PRESETS and the slots carry
   numbers. The word is in the right click menu, and a capture glyph replaces the number of an
   empty slot under the pointer. At rest, nothing says that this is where a capture happens.
   `01-at-rest-dark.png`

2. **What a capture would take is nowhere on screen before the click.** Whether the scope is every
   writable method or the pinned ones, and how many that is, is in the hover text of an empty slot
   alone (`app.rs`, the `on_hover_ui` of `preset_slots`). A full slot's hover text does not say it
   at all. `01-at-rest-dark.png`

3. **A filled slot does not say what it holds.** Sixteen squares carry sixteen numbers. A filled
   one differs from an empty one by a brighter border and a brighter number, which says that it
   holds something and nothing more: not how many values, not from which server.
   `03-filled-and-empty-dark.png`, `04-filled-and-empty-light.png`

4. **A slot does not say which server it came from.** A slot captured on Alpha and a slot captured
   on Beta are drawn identically, so recalling the wrong one onto the wrong server is invisible
   until after it has been sent. `03-filled-and-empty-dark.png`

5. **The one line that names the other server is the part that gets cut.** After a recall across
   servers the note reads "Slot 03 recalled: 4 sent, 8 not found on this server (captured on
   Alpha)", and at 1280 points the status bar shows "... 8 not found on this ser...". The
   provenance, which is the whole point of the message, is the first thing truncated.
   `06-recall-other-server.png`

6. **The note after a capture reads "12 values of methods captured".** With nothing pinned the
   scope word is empty and the sentence keeps the "of" that introduced it.
   `02-after-capture-dark.png`

7. **A capture replaces a full slot with no confirmation and no trace.** Before and after are the
   same picture; only the transient note differs. `07-overwrite-no-confirmation.png`

8. **A slot cannot be emptied.** The context menu offers Recall and Capture. Nothing anywhere
   removes a slot, so sixteen slots filled once stay filled for the life of the config.
   Proved by the code: no `presets.remove` or `presets.clear` exists in `crates/`.

9. **A slot holds no time.** `Preset` has `server`, `server_name` and `values`. Which of two
   slots is the newer cannot be answered by the window or by the file.

10. **The same gesture means two different things.** A plain click on a slot captures or recalls
    depending on whether the slot holds values, which the slot shows only as a change of border.
    No image can show this: it is a gesture, and the hover text that explains it needs a pointer
    the screenshot harness does not have.

11. **A captured value may be one the server never confirmed, and the slot does not say so.**
    Proved by `presets::tests::capture_keeps_writable_values_and_counts_the_rest`. Everywhere else
    the window distinguishes a value sent from a value read (law 1); a preset does not.

12. **The skipped count names the wrong reason.** A method is left out either because it has no
    value or because its type cannot be kept, and the note says "N without a value skipped" for
    both. A blob with a value is reported as having none. Proved by `presets::value_of`, which
    returns `None` on both paths, and by the note built in `Action::Capture`.

One defect found while auditing, not a readability point, is recorded in `PITFALLS.md` under
Config: two saves of the config in one frame lose the race, and the status bar then shows
"config not saved to ..." in place of what the capture just did.

## Three designs for the capture

All three keep the sixteen square slots, four to a row, the number alone on each. That is the
human's decision of 2026-11-17 (PRD 9.9) and it is not what was reported unclear. What they
change is the capture around the grid.

They are drawn by the real window, in the real style, against the same two servers, with slots 1
and 11 captured on Alpha and slot 4 on Beta. They are drawings: their controls do nothing, and
nothing they show is kept.

```
OSCQ_INSPECTOR_PRESET_DESIGN=a|b|c   (needs the screenshot feature)
```

The section as it ships, for comparison: `shipped-dark.png`, `shipped-light.png`.

### A, the capture bar

`a-dark.png`, `a-light.png`, and with nodes pinned `a-dark-pinned-scope.png`.

One control above the grid, the width of the pane, carrying the glyph, the word CAPTURE and the
scope it would take right now: "12 WRITABLE", or "3 PINNED" when nodes are pinned. It is meant to
be armed by a click and to take the next slot clicked, which leaves a plain click on a slot always
meaning Recall.

Answers 1, 2 and 10. Leaves 3, 4, 8 and 9 as they are. The smallest change of the three, and the
only one that does not touch the grid.

### B, the slot line

`b-dark.png`, `b-light.png`.

The grid as it is, and under it one line naming what the slot under the pointer holds, "04  4
VALUES, BETA", with CAPTURE, RECALL and CLEAR beside it in words, acting on that slot.

Answers 1, 2, 3, 4, 8 and 10, and gives slot 9 somewhere to live if a time is added. Costs one
line and one row of controls, and the facts still need the pointer to move over a slot, so the
grid at rest is no more readable than it is today.

### C, the marked slot

`c-dark.png`, `c-light.png`.

The capture bar of A, and a slot that carries what it holds: its number, the number of values
under it, and the server's name under that, in a square grown from 40 to 48 points. Alpha and
Beta can be told apart across the grid without the pointer moving.

Answers 1, 2, 3, 4 and 10, and is the only one that answers them at rest. Costs the most room,
puts three lines in a square the human chose for a number alone, and cuts a server name to five
characters, so two servers whose names begin alike are still not told apart.

## What this audit does not establish

- Nothing here was measured against a real application. Both servers are synthetic servers of
  this repository, uniform in type and access, and they cannot show what a capture does on a tree
  with impulses, blobs, write only nodes or per argument types. Points 11 and 12 rest on unit
  tests and on reading the code, not on a window.
- The hover texts, which are where the slot's facts live today, could not be photographed: the
  harness has no pointer. They are quoted from `app.rs`.
- The config race of `PITFALLS.md` was reproduced through the harness, four captures in one
  frame. The path a human would take to it, a capture clicked just after a window resize, was not
  reproduced.
- Whether any of the three designs is better than the others for the human's own work is not
  something an audit can answer. That is the choice this document is for.

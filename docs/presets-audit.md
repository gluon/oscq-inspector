# The preset section, audited from the window, and what was changed

Written on 2026-11-19 on branch `void/presets-icon`, from `c6bb5b7` (0.4.2). It answers one
report: the capture part of the preset section is not clear.

The audit below is of the section as 0.4.2 draws it, and every image of it is of that build.
The section that follows records the three designs drawn from the audit, the human's decision of
the same day, and which of the twelve points it answered. What the application does now is in
PRD 9.9 and in the README; this document is how it got there and what is left.

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

## Three designs for the capture, and what was chosen

Three were drawn, all keeping the sixteen square slots of the human's decision of 2026-11-17:
**A**, a capture bar above the grid naming the scope; **B**, the grid with a line under it
describing the slot pointed at, and CAPTURE, RECALL and CLEAR in words; **C**, a slot carrying
its count and its server, with the bar of A above. They are in
`docs/images/preset-designs/`, beside `shipped-dark.png` and `shipped-light.png`.

The human took none of the three as they stand. His decision of 2026-11-19, which is what the
application now does:

- the gestures of Max's `preset` object, click to recall, shift click to capture, shift option
  click to clear, and no confirmation on any of them,
- the marked slot of design C, its count and its server under its number,
- one line under the grid naming the three gestures and the scope, which is design B's line
  reduced to what it says rather than what it does.

The drawings were drawings: nothing was wired to the config and their controls did nothing. With
the choice made, the module that drew them is deleted; the images above and this section are the
record.

The before and the after of the section are in `docs/images/presets-change/`:
`before-dark.png` and `after-dark.png`, the same in light, `after-dark-pinned-scope.png` for the
line when nodes are pinned, and `after-dark-cleared.png` for a slot emptied. The same two servers
and the same three slots in each.

`after-dark-cleared.png` is the real binary clearing slot 04 of three filled ones, through
`OSCQ_INSPECTOR_SHOT_DO=capture:1,capture:4,capture:11,clear:4`, and the config it wrote holds
slots 1 and 11 and nothing else. What no image here can show is the gesture itself: the harness
has no pointer and holds no key, so the shift and the option come from the headless test and not
from this window.

### Which of the twelve points this answered

| Point | Answered | How |
| --- | --- | --- |
| 1, the word Capture is nowhere | yes | the line under the grid |
| 2, the scope is nowhere before the click | yes | the same line |
| 3, a filled slot does not say what it holds | yes | its count under its number |
| 4, a slot does not say which server it came from | yes | its server under its count, cut to the square |
| 5, the provenance is what gets truncated | no | the status bar is unchanged; a long note still cuts |
| 6, "12 values of methods captured" | yes | "12 writable methods captured" |
| 7, a capture replaces with no confirmation | by decision | no confirmation, stated in PRD 9.9 as intended |
| 8, a slot cannot be emptied | yes | shift option click, and **Clear** in the menu |
| 9, a slot holds no time | no | `Preset` still has no field for one |
| 10, one gesture means two things | yes | a click recalls, and says so on a slot holding nothing |
| 11, a value sent and never confirmed is not marked | no | the slot still does not say which of its values were unconfirmed |
| 12, the skipped count names the wrong reason | yes | it now names both reasons, since it cannot tell them apart |

Points 5, 9 and 11 are untouched and stay on the list.

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
  something an audit can answer. That was the choice this document was for, and it was made on
  2026-11-19.
- The gestures were proved in the headless harness, which moves the modifiers with
  `Event::ModifiersChanged` as a window system does, and on screen only through the screenshot
  harness, which has no pointer and holds no key. No one has yet shift clicked a slot on this
  Mac with a hand: the acceptance check for it is not written.

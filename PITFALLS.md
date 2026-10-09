# Pitfalls

Traps met on this project, each with the case that produced it.

## egui

A `DragValue` or `Slider` being text edited reports `changed()` on every keystroke, even with `update_while_editing(false)`, while the value itself does not move. Deciding a send on `changed()` sent the old value once per character. Decide on the value: compare it with its value before the widget was drawn.
Case: typing 0.75 into `/test/my_float` sent four messages of 0.0 before the 0.75, seen in the UDP log of the real window under Xvfb on 2026-10-09.

A plain click on a `DragValue` ends a drag: `drag_stopped()` is true with nothing changed. Sending the final value on every `drag_stopped()` sent the current value on each click.
Case: same session, two messages of 0.0 from two clicks.

The value sent is no evidence of a drag in progress. On a node that cannot be read it stays shown until the next send, so a release rule "send if a sent value is pending" sends it again on every click. Track the drags that sent.
Case: keeping the sent value on OSCQuery Helper's write only nodes, 2026-10-09.

A scroll area that becomes scrollable starts egui's scroll bar fade in, which asks for a repaint on each of the next frames, 13 with the default style. An idle repaint check run right after loading a tree counts those frames. Let the animation end, then count.
Case: `budget_idle_repaints_with_50_listens` found 8 repaints in 100 frames, all caused at `egui/src/containers/scroll_area.rs:753`, 2026-10-09.

A headless frame through `Context::run_ui` must consume `FullOutput::textures_delta`, as a renderer would, or the test panics in debug when the delta is dropped.

## Linux runtime

winit loads `libxkbcommon-x11.so.0` at run time on X11. A machine without `libxkbcommon-x11-0` panics at window creation; the build does not need it. A software Vulkan driver (Mesa lavapipe) is enough to run the window under Xvfb.

With its default backends, wgpu on Linux initialises both Vulkan and GL when both are present, and the resident memory of the window grows by about 20 MB. Memory measured under Xvfb and Mesa is dominated by the software renderer, not by the app.
Case: 166 to 169 MB with the defaults, 145.8 MB with `WGPU_BACKEND=vulkan`, 130.7 MB with no tree loaded, 2026-10-09.

## Tests

`cargo test -- --skip NAME` skips every test whose name contains `NAME`. `--skip ipv6_only` also skipped `ipv6_only_advertisement`. Use `--exact` or run the skipped one explicitly.

## OSC decoding

`rosc` 0.11 decodes nested bundles by recursion, and a deeply nested `OscType::Array` is dropped by recursion. A hostile binary WebSocket frame of 4 MB can nest bundles 200 000 deep and overflow the stack. Check the nesting without recursion before decoding.
Case: `ws::tests::hostile_nesting_is_refused_without_recursion`, run on a 256 KiB stack.

## OSCQuery servers

OSCQuery Helper declares ACCESS 2, write only, on every node and answers `?VALUE` with `{}`. Clearing the sent value on any answer to the refresh that follows a send made the widget jump back to its old position after each release.
Case: `/test/my_longlong` on the human's Mac, 2026-10-09.

Max 9 answers `/?HOST_INFO` with its whole address space. Parsing it as a `HOST_INFO` happened to work, every key being absent, but put the whole tree into the unknown keys. Detect it by `FULL_PATH`, never a `HOST_INFO` key.
Case: `tests/fixtures/max-hostinfo.json`, identical to `max-root.json`, 2026-10-09.

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

A headless test cannot prove that the window does not repaint when idle: it has no operating system, no window events, no real time. Only the real binary can, by counting its frames.
Case: the headless test found 0 idle repaints while the real binary did 563 frames in 10 s on the human's Mac, 2026-10-11.

egui animates tooltip and scroll bar fades over frames, and before showing a tooltip asks for a frame on every frame until the pointer is perfectly still. Every pointer move over the tree, where every row name has a tooltip, cost a dozen frames or more of a window at rest. Set `animation_time` to 0 and `show_tooltips_only_when_still` to false.
Case: 57 and 65 frames in 10 s on the human's Mac, causes in `tooltip.rs`, `scroll_area.rs` and `context.rs:538`, 2026-10-12.

Each input event costs at least 3 frames in egui: the event's, the one `wants_repaint_after` asks for after any input (`context.rs:538`), and the second pass egui runs after every immediate request. An idle measurement with the pointer resting on the window counts every tiny move.

A measurement taken with other servers running on the same machine measures them too. Close Max and OSCQuery Helper before measuring idle CPU or repaints.
Case: 563 frames and 21.36 % CPU with Max and the Helper running, 57 frames and 1.50 % without, 2026-10-11 and 2026-10-12.

`Context::repaint_causes()` lists where the code asked for the frame. A frame woken by the operating system or by input, a window event, has no cause listed. An empty list is therefore a finding, not a lack of one.

A headless frame through `Context::run_ui` must consume `FullOutput::textures_delta`, as a renderer would, or the test panics in debug when the delta is dropped.

eframe gives the window its own egui logo as icon when the application gives none: window, taskbar, and on macOS the Dock while the app runs, over the bundle's `.icns`. Give the icon in the `ViewportBuilder`.
Case: `load_default_egui_icon` in `eframe/src/native/epi_integration.rs`, read while adding the icon, 2026-10-12.

`Ui::scope_builder` with a `max_rect` places a child and then moves the parent's cursor past it, wherever that rect was. Drawing the three cells of a row with it moved the cursor back after each row: rows overlapped every 12 px. Cells drawn at a rect of their own, without moving the parent, go through `Ui::new_child`.
Case: the M6 three column tree, under Xvfb, 2026-10-13.

A scroll area's content can be wider than what is visible of it, and `available_width()` inside it gives the content's width. A cell aligned to the right of that width is drawn off screen: no error, nothing visible. Take the visible width from the clip rectangle.
Case: the M6 metadata column, 0 of the expected `rw` labels drawn, because a header wider than the central panel made its content overflow under the right panel, 2026-10-13.

A side panel sized by its content grows a little on every frame when a child asks for `available_width()` plus anything more. The server pane widened by 3 px a frame: the remove button needed 28 px and 24 were reserved for it.
Case: same session.

A row laid out with `Ui::horizontal` does not wrap: past the width of its pane it runs on, under the next pane, clipped, with no error. A toolbar sized on a 1100 px window lost its last controls on a narrower one. Use `horizontal_wrapped`, and a text field, which shrinks to the end of the line rather than wrap, needs `end_row` first when the line is short.
Case: Refresh and the Tree tab out of sight on the human's Mac after the M6 rework, 2026-10-13.

Side panes left at egui's default size range take what they want and the central pane gets the rest, down to nothing when they are dragged wide. Give each a range computed per frame from the window width, the second from what the first actually took.
Case: same day.

`TextShape::visual_bounding_rect` is the bounds of the tessellated glyphs: in a headless frame it is a sliver, not the text. To test where a text lies on screen, use `galley.rect` moved by the shape's `pos`.
Case: the toolbar visibility test, 2026-10-13.

A side panel in egui 0.36 gives its content only the least width of its size range and is painted as wide as that content, from its left edge; its size is stored only on release, from what was painted. A right panel holding little, dragged wider, was painted short with the rest of the window left black, then went back to its least width on release. Make the content fill: `ui.set_min_width(ui.available_width())` first thing in the panel. A test that reads `PanelState` during a drag sees the old size: check what was painted.
Case: the work pane with nothing pinned, on the human's Mac, 2026-10-13.

`egui::Modal` is sized on its first frame and drawn on the next: a headless test reads its texts on the second frame.

`time::UtcOffset::current_local_offset` fails on Linux and macOS as soon as the process has a second thread. Read it first thing in `main`, before the async runtime or the window start.

On macOS, winit gives the application a default menu whose first item, "About" and the process name, opens the system's standard about panel, not anything the application draws. That panel reads the bundle: run outside one, with `cargo run`, it shows a generic folder icon and the process name. A test of the application's own about box says nothing of it. Point the item at the application's box.
Case: "the about box is empty" on the human's Mac, 2026-10-13, while the box drawn by the application passed its test.

A test of a window opened by setting its state directly does not cover the way a user opens it. Open it as the user's entries do.

In `Ui::horizontal_wrapped`, `available_width()` is the width of the whole line, not what is left of it after the cursor. Sizing a field from it sent the toolbar's fields to the next line on a window where they fitted. What is left is `max_rect().right() - cursor().min.x`.
Case: the v0.2.1 toolbar, 2026-10-13.

`Galley::text()` returns the text a label was given, whole, even when egui elides it on screen. A test comparing what the user sees must read the glyphs laid out (`galley.rows`, each glyph's `chr`).
Case: a filter test that kept passing with labels cut to the same text on screen, 2026-10-13.

egui fills buttons and text fields with `weak_bg_fill` but slider rails and checkbox boxes with `bg_fill`, both from the same widget state. Retoning a theme through one of them leaves the other widgets as they were.
Case: the v0.2.2 light theme, white buttons and fields over grey rails, 2026-10-13.

An unselected `toggle_value` is drawn with no frame, like a plain label: nothing tells a user it can be clicked. A `checkbox` shows its state either way.
Case: the "Writable only" control, under Xvfb, 2026-10-13.

## Status display

An error kept until another error replaces it outlives what it described. The status bar showed a failed connection under the tree the next connection had loaded. Clear an error when the operation it comes from succeeds, and drop the errors of an attempt that a newer one replaced.
Case: "could not connect to http://localhost:30339/" next to 35 nodes and an open WebSocket, Max 9 on the human's Mac, 2026-10-12.

A reqwest error displays only the step that failed, "error sending request"; the reason, refused, no route, DNS, is in its sources. A message built from `to_string()` alone tells the user nothing they can act on. Walk the sources.
Case: "could not connect to http://localhost:30339/: error sending request" on the human's Mac, with Max answering curl, 2026-10-12.

A failure to reach a server is not something the server declared. Clearing its `HOST_INFO` on an unreachable attempt turned its read write nodes into write only or blocked rows, labelled "read only".
Case: the reconnection to Max 9 failing under a loaded tree, 2026-10-12.

Commands that go out on a reload are not churn until the rows on screen are shown to be the same. A test document that replaces a subtree with part of it changes the rows, and the `IGNORE` then `LISTEN` it shows is the listen discipline doing its job. Count the rows before and after each event.
Case: 14 `IGNORE` and 14 `LISTEN` in `a_sent_value_stays_shown_on_a_write_only_node`, taken for churn, were `/test` going from 15 rows to 1 and back, 2026-10-12.

## Linux runtime

winit loads `libxkbcommon-x11.so.0` at run time on X11. A machine without `libxkbcommon-x11-0` panics at window creation; the build does not need it. A software Vulkan driver (Mesa lavapipe) is enough to run the window under Xvfb.

With its default backends, wgpu on Linux initialises both Vulkan and GL when both are present, and the resident memory of the window grows by about 20 MB. Memory measured under Xvfb and Mesa is dominated by the software renderer, not by the app: the same test gives about 111 MB on macOS against 166 MB in the container.
Case: 166 to 169 MB with the defaults, 145.8 MB with `WGPU_BACKEND=vulkan`, 130.7 MB with no tree loaded, 2026-10-09.

## Tests

`cargo test -- --skip NAME` skips every test whose name contains `NAME`. `--skip ipv6_only` also skipped `ipv6_only_advertisement`. Use `--exact` or run the skipped one explicitly.

The test filter is a substring too. A real binary test named `budget_cold_start...` was picked up by the in process `budget_` filter and failed in CI for want of a display. Real binary tests are named `real_binary_...`.
Case: CI run 38208380933, 2026-10-12.

`pkill -f PATTERN` matches its own shell when the pattern is in the command line, and kills it. Stop processes by the pid recorded when they were started.
Case: killing the synthetic server by name ended the session's shell, 2026-10-12.

## OSC decoding

`rosc` 0.11 decodes nested bundles by recursion, and a deeply nested `OscType::Array` is dropped by recursion. A hostile binary WebSocket frame of 4 MB can nest bundles 200 000 deep and overflow the stack. Check the nesting without recursion before decoding.
Case: `ws::tests::hostile_nesting_is_refused_without_recursion`, run on a 256 KiB stack.

## OSCQuery servers

OSCQuery Helper declares ACCESS 2, write only, on every node and answers `?VALUE` with `{}`. Clearing the sent value on any answer to the refresh that follows a send made the widget jump back to its old position after each release.
Case: `/test/my_longlong` on the human's Mac, 2026-10-09.

Max 9 answers `/?HOST_INFO` with its whole address space. Parsing it as a `HOST_INFO` happened to work, every key being absent, but put the whole tree into the unknown keys. Detect it by `FULL_PATH`, never a `HOST_INFO` key.
Case: `tests/fixtures/max-hostinfo.json`, identical to `max-root.json`, 2026-10-09.

An accepted WebSocket, and an accepted `LISTEN`, do not mean the server notifies. Max 9 accepts both on its HTTP port and never sends a frame. Showing "live" on open told the user values were pushed when none ever would be. Claim live only after the first frame from the server.
Case: Max 9 on the human's Mac, `LISTEN /test/param/fader/raw`, fader moved, nothing on the socket, checked with `websocat`, 2026-10-11.

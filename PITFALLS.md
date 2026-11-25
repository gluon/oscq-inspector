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

egui wakes the window one predicted frame before the delay asked for: `request_repaint_after` subtracts `predicted_dt`. A timer that acts only once its time has come is found not due by the frame that woke for it, which asks again for the few milliseconds left, and a delay of zero costs two frames. Two or three frames per interval where one was meant. Let the action fall due within two predicted frames of its time.
Case: `the_peers_timer_runs_only_while_the_map_is_shown`, the rest of the interval asked for was 1.4833 s, not 1.5, 2026-11-13.

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

`egui::RawInput` carries no modifier field: `InputState::modifiers`, which a widget reads to tell a shift click from a plain one, is moved by `Event::ModifiersChanged` and by nothing else. A test that fed a `PointerButton` event with `modifiers: SHIFT` set left `InputState::modifiers` empty, so the widget saw a plain click and the shift route went untested while the test passed. Announce the key going down before the click and up after it, as a window system does.
Case: the shift click that captures into a full preset slot, caught by the mutation that removed the route, 2026-11-17.

`Context::read_response(id)` falls back to the pass before when this pass holds no such widget. A test that removed a control and read its place back one frame later still got the old rectangle and concluded the control was still drawn. Run two frames after a change before reading a response back.
Case: `a_manual_entry_is_removed_with_its_osc_port_a_discovered_one_is_not`, reading the icon button of a removed manual entry, 2026-11-17.

A control that carries no text, an icon button, cannot be found in a frame's shapes by its label, and `allocate_exact_size` gives it an id of its own that a test cannot rebuild. Interact with a whole `Id` the call site names, and read the response back from the context by it; check the strokes painted inside that rectangle, or a button that paints nothing passes.
Case: `glyph::id_of`, and the mutation that makes `button_sized` paint no icon, 2026-11-17.

A theme's fill chosen for one purpose may collide with another's. `extreme_bg_color` and the ground of a button at rest became one grey, and a test that found the map's node boxes by `fill == extreme_bg_color` counted the theme box and About in the server pane's footer as two more nodes. Tell a region by its place as well as its fill when a fill is shared.
Case: `a_server_moved_by_hand_is_drawn_once`, 8 nodes found for 6, 2026-11-17.

## Status display

A count is not a name. The status bar said `N warnings` and no hover gave them, so nothing on screen said which attribute a server had sent unusably, or what it held: a `WS_IP` of `""` was dropped with no trace in the window. The diagnostics of `HOST_INFO` did not even reach the window, `connect` taking them as `Ok((info, _))` and dropping them, so that count only ever covered the address space document. Carry the diagnostics to the window and give one line per warning.
Case: the REAPER server of 2026-11-23, and `a_counted_warning_is_named_on_hover_with_its_attribute_and_its_value`.

A zone built inside `if let Some(tree)` says nothing in the very case its reader needs it: the warning count of a server whose tree never loads, a `HOST_INFO` having been read long before any tree was asked for. Read what a zone depends on, not only what it draws.
Case: the same empty `WS_IP`, counted only once a tree had loaded, and `a_host_info_warning_shows_with_no_tree_and_its_count_is_written_in_words`.

`HostInfo::extra` and `Node::extra` hold two different things under one name: the server's unknown keys, and its known keys whose value had the wrong JSON type. A window reading `extra` to tell which attributes were dropped marks an extension the server legitimately published as a fault. Only the parser's diagnostics say which key was dropped; `extra` says what it held.
Case: the same change, whose first design was to name the keys of `extra`.

An `egui::Label` with `truncate()` shows its whole text in a tooltip of its own when it is cut, `show_tooltip_when_elided` being true by default. A cut label given `on_hover_text` as well shows two tooltips, one over the other. Turn one off.
Case: the status bar's message, the reason of a failed `?PEERS` drawn twice on hover, found by `the_whole_reason_of_a_failed_peers_reading_reaches_the_status_line`, 2026-11-14. The other zones of the status bar cut their labels and add a hover of their own; not checked.

An error kept until another error replaces it outlives what it described. The status bar showed a failed connection under the tree the next connection had loaded. Clear an error when the operation it comes from succeeds, and drop the errors of an attempt that a newer one replaced.
Case: "could not connect to http://localhost:30339/" next to 35 nodes and an open WebSocket, Max 9 on the human's Mac, 2026-10-12.

A reqwest error displays only the step that failed, "error sending request"; the reason, refused, no route, DNS, is in its sources. A message built from `to_string()` alone tells the user nothing they can act on. Walk the sources.
Case: "could not connect to http://localhost:30339/: error sending request" on the human's Mac, with Max answering curl, 2026-10-12.

A failure to reach a server is not something the server declared. Clearing its `HOST_INFO` on an unreachable attempt turned its read write nodes into write only or blocked rows, labelled "read only".
Case: the reconnection to Max 9 failing under a loaded tree, 2026-10-12.

Commands that go out on a reload are not churn until the rows on screen are shown to be the same. A test document that replaces a subtree with part of it changes the rows, and the `IGNORE` then `LISTEN` it shows is the listen discipline doing its job. Count the rows before and after each event.
Case: 14 `IGNORE` and 14 `LISTEN` in `a_sent_value_stays_shown_on_a_write_only_node`, taken for churn, were `/test` going from 15 rows to 1 and back, 2026-10-12.

## Logging

`tracing_subscriber::EnvFilter::from_default_env()` falls back to `error` when `RUST_LOG` is not set, not to `info` nor to `warn`: tracing-subscriber 0.3.23, `filter/env/mod.rs`, `from_default_env` calls `with_default_directive(LevelFilter::ERROR.into())`. Every failure the application logs at `warn` was dropped on a plain launch. Build the filter with an explicit default directive.

A failure shown in the window is not therefore in the log. The map showed a failed `?PEERS`, cut to the width of its node, and nothing anywhere logged it: not a level problem, no line was ever written. Log every failure where it is produced, whole, naming its server.
Case: the map showed `peers not read: invalid PEERS reply: L...` against a real server, and `grep -i peers` on the log at `RUST_LOG=info` found nothing, the human's Mac, 2026-11-13.

A test that captures the log with a dispatcher set on its own threads (`tracing::dispatcher::set_default`) passes alone and finds the log empty beside other tests: a callsite whose interest is first computed on a thread with no dispatcher can cache "never" for every thread. A log belongs to a process. Read it in a process of its own: the test starts its own binary on one ignored test, with the environment it wants, and reads its output.
Case: `a_failed_peers_reading_is_logged_whole_without_rust_log`, green with `cargo test NAME`, an empty log under `cargo test`, 2026-11-14.

## Linux runtime

winit loads `libxkbcommon-x11.so.0` at run time on X11. A machine without `libxkbcommon-x11-0` panics at window creation; the build does not need it. A software Vulkan driver (Mesa lavapipe) is enough to run the window under Xvfb.

With its default backends, wgpu on Linux initialises both Vulkan and GL when both are present, and the resident memory of the window grows by about 20 MB. Memory measured under Xvfb and Mesa is dominated by the software renderer, not by the app: the same test gives about 111 MB on macOS against 166 MB in the container.
Case: 166 to 169 MB with the defaults, 145.8 MB with `WGPU_BACKEND=vulkan`, 130.7 MB with no tree loaded, 2026-10-09.

## Tests

A real binary idle test on a machine with a live local network measures that network too. A server announced or expired over mDNS reaches the window through the notify callback, and the window draws it, rightly: 11, 21 and 11 frames in a ten second window where the timer beat five times, every extra frame caused at `app.rs:233`, one of those windows holding 1 `ServerFound` and 3 `ServerStale`. What hid it: the same test had just passed on the same machine, and a frame count names no cause; only `repaint_causes` and the kinds of the events do. The test now refuses such a window as an invalid measurement instead of reporting a budget overrun, and measures again; after three refusals in a row it fails on the machine, since a test that only ever refuses guards nothing.
Case: `real_binary_map_visible_repaints_once_per_poll_interval` on the human's Mac, 2026-11-13; reproduced by announcing one service with `dns-sd -R "name" _oscjson._tcp local 5678` while the window was open.

A duration measured on the macOS CI machine strays more than 5 percent from the one asked for: a 2000 ms timer has been seen delivering one frame 112 ms late and the next 110 ms early. The Linux CI machine and the human's Mac print 2.000 s every time, under a millisecond. A test that asserts a duration with a small tolerance therefore passes on both of those and on four runs out of five there, which is what hid it: the strays before were 53 to 56 ms, under the 100 ms floor the test allowed. Assert what the application decides, a count of frames over a window or the span of several intervals, not one interval measured once.
Case: `real_binary_map_visible_repaints_once_per_poll_interval`, gaps 1.988, 1.993, 2.112, 1.890 s against a floor of 1.900 s, run 43442955538 on `main`, 2026-11-13. The numbers of every window measured are in docs/decisions.md, "M7, the map poll tolerance".

The macOS CI machine is sometimes slow as a whole, not on one path: every budget of a run slows together. A gate on the budget with the least margin falls on such a run, and passes when the same commit is run again. Read the other budgets of the same run before reading a regression into the one that fell.
Case: `budget_slider_to_udp_packet`, run 43461452493 on `main`, `e73104e`, 2026-11-13. First attempt, job 132009140477: median 0.762 ms, p95 2.119 ms, max 24.559 ms against 2 ms, failed; in the same run parse and model median 18.841 ms, scrolling p95 2.226 ms, map frame p95 1.283 ms, the budgets step 6 min 55 s. Second attempt, no code changed, job 132078840708: slider median 0.315 ms, p95 0.389 ms, max 0.553 ms; parse median 7.359 ms, scrolling p95 0.497 ms, map p95 0.377 ms, the step 2 min 6 s. The October reference on that machine was a p95 of 1.535 ms (docs/decisions.md, M6), 23 percent under the budget, the least margin of any budget there.

A port the system gave in one protocol is not free in the other. A test bound its HTTP server on port 0, then its UDP receiver on the number TCP got, as if it were free in UDP too. Windows reserves port ranges per protocol, and draws them again at each boot (`netsh interface ipv4 show excludedportrange protocol=udp`): the number can be forbidden in UDP, and the bind fails with `WSAEACCES`, 10013, not with "address in use". Bind each protocol, never assume one from the other: the UDP receiver on port 0 first, then TCP on its number, another number if TCP refuses (`server_and_receiver_on_one_port`, `tests/common/mod.rs`). On Linux the same mistake fails as `AddrInUse` when the number is held in UDP.
Case: `osc_port_absent_sends_to_http_port`, `http.rs:94`, Windows CI, run 43606863201 attempt 1, job 132483268518, on `void/map-honesty`, 2026-11-14; green on the rerun with no change.

A port released is not a port kept. A test bound port 0 to learn a free number, let it go, connected to it expecting a refusal, then started its server on it: two assumptions, that nobody took the number between, and that it was still free for the server. Hold it instead: a TCP socket bound and not listening refuses connections and keeps the number; the server is that same socket put to listen later (`held_port` and `SyntheticServer::start_ws_on`, `tests/common/mod.rs`). On Linux, a number released then taken by another process fails the old way with `AddrInUse`; held, the other process's bind fails instead, and a connection is refused. Not on macOS: there a connection to a socket bound and not listening is not refused, it waits unanswered until the client gives up, so a test that holds a port must accept the timeout as the failure as well as the refusal.
Case: `a_failed_connection_is_not_shown_once_a_new_one_succeeds`, the class of `osc_port_absent_sends_to_http_port`, 2026-11-14; on macOS, `no answer from http://127.0.0.1:49391/ within the timeout`, CI run 43645762353.

`cargo test -- --skip NAME` skips every test whose name contains `NAME`. `--skip ipv6_only` also skipped `ipv6_only_advertisement`. Use `--exact` or run the skipped one explicitly.

The test filter is a substring too. A real binary test named `budget_cold_start...` was picked up by the in process `budget_` filter and failed in CI for want of a display. Real binary tests are named `real_binary_...`.
Case: CI run 38208380933, 2026-10-12.

A test that reads a text of the map reads it whatever size it was drawn at. Four tests read node titles and edge labels the default window drew under 9 points, and passed: the text was in the shapes, nobody could read it on the screen. A text has a size on screen, the size it was laid out at times the scale of its layer; read that too.
Case: four map tests failed once the map stopped drawing text under 9 points, 2026-11-14.

A test that every drawn text is readable passes when nothing is drawn. The rule "no text under 9 points" and its test held for the map of eight servers and 24 peers while it drew no text at all, and so did the maps of two and three servers in the default window. A rule of what may not be drawn needs beside it what must be: here, every server's name.
Case: `after_fit_eight_servers_and_24_peers_take_the_canvas_and_no_label_is_unreadable` green with "texts 0" in its own output, on `main` at `5161db3`; the human saw only boxes, 2026-11-14.

A test of a computation that has a fallback passes on the fallback. `the_places_are_computed_and_no_node_covers_another` would stay green with a placement by forces that always left nodes overlapping, since the ellipses of M7 take over then and never overlap. Count which one ran before reading the green: a trace on each branch, the six formats run, the trace removed.
Case: the placement by forces, 2026-11-14: counted, 0 fallbacks over the six formats and the compact cases, `docs/decisions.md`.

A rule checked in one mode of a drawing holds only there. Labels were kept off nodes in the compact map, written when the full map's ellipses left the middle of every edge empty; the placement by forces puts a node anywhere, and a label was drawn over its title. When what makes a rule unnecessary changes, check the rule everywhere.
Case: a "no packet" label over Spatialisation and over this application's node, the human's capture at 1600 by 1000, 2026-11-14; reproduced by `an_edge_label_never_covers_a_node`.

A criterion on the box that holds a drawing says nothing of its shape. The map of a centre and three neighbours drawn on one line, two nodes alone in the bottom corners, covered the canvas: the alone did. Measure the shape itself: the gaps around a node, the spread on the short axis.
Case: the map after Release, 1600 by 1000, 2026-11-14; locked by `a_centre_and_its_neighbours_spread_around_it_not_on_a_line`.

A threshold set where a measure of the defect lands is no threshold. A largest gap of 180 degrees allowed was proposed, and the placement with its defect measured 180.0 exactly. Measure the defect and the fix, and set the bound between them, nearer the fix.
Case: the same test, 2026-11-14, `docs/decisions.md`.

A thing drawn twice on screen may be one thing and its hover. egui shows a tooltip under a pointer at rest, and a pointer is at rest on what it has just dragged. A map node whose hover repeated its box looked drawn twice, and only after a drag; counting nodes, or drawing with the pointer elsewhere, never shows it. Reproduce with the pointer where the hand leaves it, a pixel moved, time passing.
Case: Plancher drawn twice with positions set by hand, the human's screen, 2026-11-14; the hypothesis of a key mismatch did not reproduce, the hover did, `a_server_moved_by_hand_is_drawn_once`.

A test that names a tree row by the leftmost text at its height takes a container's expander, `-` or `+`, for its name.
Case: `drawn_badges` reported the badge of `/foo` on a row named `-`, 2026-11-13.

A request still on its way is not asked again, so asking right after a change can bring back the answer to a request made before it. A test that waits for a reading to reflect a change must ask again until one does, as a user pressing Refresh would.
Case: `three_synthetic_servers_are_read_drawn_and_badged_through_the_real_service`, the reading asked for the tree arrived without the listens sent meanwhile, 2026-11-13.

A real binary test opens its window where the person at the machine is working: their pointer and keys reach it, and an idle measurement is then refused as invalid. Run them when the machine is left alone, or in CI.
Case: `real_binary_map_frame_time`, 72 then 892 input events in the idle window, 2026-11-13.

`pkill -f PATTERN` matches its own shell when the pattern is in the command line, and kills it. Stop processes by the pid recorded when they were started.
Case: killing the synthetic server by name ended the session's shell, 2026-10-12.

`real_binary_map_frame_time` measures nothing unless its first two windows were disturbed. It reads frames `since = start + IDLE_WINDOW * (MEASUREMENT_ATTEMPTS - 1)`, that is 20 s after the measurement began, clamped by `.min(Instant::now())`; a run whose first window is accepted lasts about 10 s, so `since` is clamped to the present and no frame is at or after it. It then fails on `assert!(n >= 20, "only {n} frames measured")`. It passes only when the retries pushed the run past 20 s. It sits in the CI step marked `continue-on-error: true`, which is why it went unnoticed.
Case: "only 0 frames measured" on the human's Mac on 2026-11-17, and the same on d3df218, the state before `void/visual-refresh`, so it is not that branch's doing. Not fixed there: the branch was for visuals.

The pinned panel asks `show_rows` for rows of `ROW_H`, 24 points, and draws them 28 apart: the row is a `ui.horizontal` block `ROW_H` tall and the panel's own `item_spacing.y`, 4 points, follows each one. The virtualised list's arithmetic is out of step with what it draws by 4 points a row.
Case: measured at 28 on `void/visual-refresh` and at 28 on d3df218 by the same probe, 2026-11-17, so it is older than that branch. Left as it is there, and held where it is by `the_pinned_rows_keep_one_row_height_each_hairline_and_all`, so it cannot grow unnoticed.

The Traffic tab's own toolbar does not wrap and its filter field asks for a fixed 192 points, while the centre pane keeps only 224 at the 640 point minimum window. The direction filter is cut mid-word and Pause, Clear and Export are off screen entirely. `every_toolbar_control_stays_on_screen_whatever_the_width` does not cover it: it checks the tab bar's own toolbar, the preset slots, the pinned row and the status bar, not the toolbar a tab draws for itself.
Case: seen at 640 by 800 on the human's Mac on 2026-11-17, on `void/visual-refresh` and, in the same screenshot taken from the build of d3df218, before it: the same cut at the same place, so it is not that branch's doing. Fixed the same day, on the human's word: both rows wrap, and `every_control_a_tab_draws_for_itself_stays_on_screen` reaches what the toolbar test did not.

A test that asserts the wording of a connection failure asserts a platform. A pooled HTTP connection reused after its server has gone is reset as the request is sent, which reqwest reports as a send failure, `request to <url> failed`, and Windows as `os error 10054`; on Linux and macOS the socket has usually been closed already, so a new connection is opened, refused, and reported as a connect failure, `could not connect to <url>`. One event, two sentences, and only the second was asserted.
Case: `a_failed_connection_keeps_the_declared_access_and_says_nothing_can_be_sent`, `app.rs:8393`, windows-latest, runs 45696633313 and 45706928879 of 2026-11-25 on `void/accept-artifact-claim`, ports 62599 and 59662. Intermittent, so a green run disproves nothing: the same branch passed in run 45709367674 and the same commit `224ef23` passed on main in 45693582232. `ClientError` now reads the cause chain, `is_connection_level` in `client.rs`, and calls a reset what the other platforms call it. A separate fault of the same night, `os error 10013` on 49729 to 49760, is the entry below; the two are unrelated beyond both being Windows and both being sockets.

Thirty-two tries at a port can all land in the same forbidden band. The pair helper binds UDP on port 0 and tries TCP on the number it was given, retrying when TCP refuses, which is the entry above's remedy. Windows draws its TCP exclusions as contiguous ranges and does not take them out of the UDP ephemeral pool, so UDP kept handing out consecutive numbers inside one and every attempt failed with `os error 10013`, PermissionDenied, on 49729 to 49760: thirty-two numbers, exactly the width of the loop. A retry that walks forward one number at a time cannot leave a band as wide as its own attempt count, so the count was never the fix.
Case: `osc_port_absent_sends_to_http_port` panicking at `tests/common/mod.rs:517` with the whole list of thirty-two refusals, and `a_port_refused_in_tcp_is_left_for_another` at `http.rs:130`, where the test's own `TcpListener::bind` hit the same 10013 on the number it had just been handed. Run 45693582232 attempt 1, job 139485067542, windows-latest, 2026-11-25. Green on the rerun with no change, and `gh run view` without `--attempt 1` shows only that rerun: a run reported as passing can hold a failure, and reading the latest attempt is not reading the run.

## macOS local network

macOS decides whether an application may reach the local network per build, not per bundle identifier. A Rust binary is ad hoc signed by the linker, so every build has its own code identity: `codesign -dvvv` on two builds of the same bundle gives two `CDHash` values and two `Identifier`s. An update is a new application to the system, its answer starts again as undecided, and Privacy & Security, Local Network lists the old entry and the new one under the same name. Keeping the bundle identifier, which this repository does on purpose, does not keep the permission.
Case: 0.4.0 installed over 0.3.0 on the human's Mac, 2026-11-18, two "OSCQuery Inspector" entries listed. Reproduced here: a byte for byte copy of a binary under a *different* bundle identifier inherited the decision and was never blocked, while a *different* binary under the *same* identifier was blocked, which is the wrong way round for an identity that followed the identifier.

While the decision is pending or denied, a multicast send fails with `EHOSTUNREACH`, "No route to host", errno 65. `mdns-sd` 0.21.5 says nothing of it: `multicast_on_intf` and `unicast_on_intf` trace the failure and return, and no `DaemonEvent::Error` is raised. A browse that has asked the network nothing therefore looks exactly like a browse that asked and heard nothing, and the window said "none found" about a network it had never reached.
Case: three launches of a probe under fresh identities, 2026-11-18, each refused for the first seconds; the log is in the branch's decision line.

`mdns-sd` doubles the gap between the queries of a browse and caps it at one hour: measured on the human's Mac at 0, 1, 3, 7, 15, 31, 63 and 127 s, and `max_delay = 60 * 60` in its source. Nothing resets it. So an answer from the system that arrives a minute into a session is not acted on for another minute or more, which is why quitting and relaunching looked like the only cure.
Case: the same probe, left running for several minutes, 2026-11-18.

A permission turned off in System Settings does not reach a process that is already running: the probe kept sending after the human switched its entry off and on again. The decision is read when the process first reaches the network, and a denial met later in a session could not be produced this way.
Case: tried on 2026-11-18 to produce a denied state for the reproduction, and abandoned.

**The refusal stops before the permission arrives.** The `EHOSTUNREACH` of an undecided build lasts a few seconds and then the system takes the sends: `send_to` returns `Ok` with the byte count, and that is all it means. Measured in an application bundle of a fresh code identity, launched by the Finder: refused at +0.00 s, taken at +2.01 s, with no dialog shown and no entry to be seen. So a send that is taken is no evidence that anything left the machine, and reading it as one is how 0.4.1 went back to saying "none found" about a network it may never have reached. Only an answer is evidence.
Case: `scratchpad/regression/probe-run.log` and `disc-head.log`, two bundles of fresh identities, 2026-11-18. A third, `disc-watch.log`, went through every state in one run, which is the whole defect and its recovery in seven lines: refused at +0.00 s, the `dns-sd` server of this Mac found on loopback at +0.18 s while the state stayed refused, the system taking the queries at +24.07 s with no dialog and nothing allowed, the second query taken with nothing answering at +29.08 s, and the server advertised on the Wi-Fi address of this Mac answering at +31.17 s, with no relaunch. The window of 0.4.1 would have said "none found" from +24.07 s, with that server advertising all along. Not distinguishable in that run: which query drew the answer, ours at +29.08 s or `mdns-sd`'s own, due at about +31 s on the schedule measured above. Where the grant lands later, in the minutes where those gaps have grown to one and two minutes, the floor is what shortens the wait, and that is held by the tests rather than by this run.

**A blocked application still hears loopback.** In that same state the bundle heard every packet announced on loopback, and a browse resolved a service registered with `dns-sd` from its loopback records alone. On the human's Mac that is exactly what the window showed: one server of his own machine, registered with `dns-sd` seconds earlier, and nothing of the server advertised on the Wi-Fi address of the same Mac, which 0.4.0 beside it listed and connected to. So "something was found" is not evidence either, unless it was found at an address that is not loopback, on an interface that is not loopback.
Case: the human's 0.4.1 at 15:28 on 2026-11-18, against 0.4.0 at the same moment; the loopback half is held by `the_local_network_is_asked_again_until_something_answers`, which registers a service at `127.0.0.1` and reads its records off a real wire.

An integration test cannot hold the half that matters on a machine whose own network answers. `MdnsSdDiscovery` browses the two real service types, so on a machine with any OSCQuery server running something answers within a second, the question is settled as it should be and there is nothing left to ask: the assertions about asking again, and about what is said while nothing answers, cannot run. They are held without a network by the unit tests of `Asking` in `crates/oscq-discovery/src/mdns.rs`, and the integration test says in words which half it skipped.
Case: `the_local_network_is_asked_again_until_something_answers` on the human's Mac with a server running, 2026-11-18: probe called once, `Heard` at +1 s.

## OSC decoding

`rosc` 0.11 decodes nested bundles by recursion, and a deeply nested `OscType::Array` is dropped by recursion. A hostile binary WebSocket frame of 4 MB can nest bundles 200 000 deep and overflow the stack. Check the nesting without recursion before decoding.
Case: `ws::tests::hostile_nesting_is_refused_without_recursion`, run on a 256 KiB stack.

## OSCQuery servers

Not reproduced. Against a real server among the eight of the human's setup, which one was not recorded, the map showed `peers not read: invalid PEERS reply: L...`, cut to the width of its node, and the log at `RUST_LOG=info` held nothing, since no line was written (fixed, see Logging). What is proved, and only that: the map drops the address of the request, so what followed `invalid PEERS reply: ` was the text of the refusal, and of every refusal the parser can build exactly one starts with `L`: one client's `LISTEN` holding more than 8192 addresses (`the_prefix_seen_on_2026_11_13_names_one_variant`). Suspected, not reproduced: that server listed a client listening to more than 8192 addresses, or listed one address several times, the bound being checked on the list as received, before the window removes repeats (`Peers::parse`, then `AppState::apply` on `Event::Peers`). What would settle it: the line the log now writes for that server, `peers not read: ...` with its `server=`, or the reply of `/?PEERS` saved when it happens. Every refusal a conforming server can cause, `CLIENTS`, `SENDERS` or `LISTEN` over their bounds, a `NAME` or a listened address over its length, the reply over its size, is now drawn whole on hover of the status bar and logged whole (`every_refusal_a_conforming_server_can_cause_is_shown_and_logged_whole`).
Case: the human's Mac, eight servers and their peers on the map, 2026-11-13; reported 2026-11-14.

The `PEERS` extension lists this application among each server's clients, under the name it sent. Another client may send the same name. Telling it from this application by name drew two clients as one; by the local address of its own socket, address and port both, it is found for certain, and only it.
Case: `this_application_is_one_node_told_from_a_namesake`, 2026-11-13.

A server that lists a client under its own name is connected to itself. A map that drops such an entry, taking it for a mistake, hides a real connection: another map known to the human does. Draw it, as a loop on the one node.
Case: the human's brief for M7, locked by `a_server_connected_to_itself_is_drawn_with_a_loop`, 2026-11-13.

OSCQuery Helper declares ACCESS 2, write only, on every node and answers `?VALUE` with `{}`. Clearing the sent value on any answer to the refresh that follows a send made the widget jump back to its old position after each release.
Case: `/test/my_longlong` on the human's Mac, 2026-10-09.

Max 9 answers `/?HOST_INFO` with its whole address space. Parsing it as a `HOST_INFO` happened to work, every key being absent, but put the whole tree into the unknown keys. Detect it by `FULL_PATH`, never a `HOST_INFO` key.
Case: `tests/fixtures/max-hostinfo.json`, identical to `max-root.json`, 2026-10-09.

An accepted WebSocket, and an accepted `LISTEN`, do not mean the server notifies. Max 9 accepts both on its HTTP port and never sends a frame. Showing "live" on open told the user values were pushed when none ever would be. Claim live only after the first frame from the server.
Case: Max 9 on the human's Mac, `LISTEN /test/param/fader/raw`, fader moved, nothing on the socket, checked with `websocat`, 2026-10-11.

A `HOST_INFO` key that is present and empty is not the same as one that is absent, and reading the two alike is not harmless. A REAPER server serves `"WS_IP":""`. Kept as the empty string, it named no host: the address became `ws://:<port>/`, which nothing can connect to, so no `LISTEN` was ever sent and the server stayed on values on refresh only. Nothing in the failure names the field or the address. The service reported `failed to lookup address information: nodename nor servname provided, or not known`, a resolver error, retried with a growing backoff, while `Url::parse` of that same string answers `EmptyHost`: the mechanism differs depending on who reads the address, and neither mentions `WS_IP`. Treat an address that is empty or spaces only as absent, and do it at parse time rather than where the address is read, so that the flag saying whether the server advertised the address cannot claim the fallback.
Case: the human's REAPER server, reported 2026-11-23, and `an_empty_ws_ip_opens_the_socket_on_the_http_host_and_a_value_comes_back`, which timed out after 11.79 s on the old code, having only reached `HttpOnly` and five `WebSocketLost`, and passes in 0.01 s.

## Config

Two saves of the config in one frame lose the race and one of them is dropped. `write_atomic`
builds the temporary file's name from the destination's, so both writers open the same
`.config.json.tmp`; the first rename carries it away and the second fails with ENOENT. The window
then shows "config not saved to ..." in the status bar, which replaces the note the action had
just put there, so a capture that worked reads on screen as a failure to save.
Case: four captures in one frame through the screenshot harness, five launches out of five, three
warnings each time, on the human's Mac on 2026-11-19. A hand does not click four slots in one
frame, but `track_geometry` saves from the same frame as the action that follows it once its
settle has elapsed, so a capture clicked just after a window resize is the human path to it. That
path was not reproduced.

Fixed the same day: each save writes its own temporary file, and a save overtaken by a newer one
drops its file rather than put back what the newer one replaced. Unique names alone would not
have been enough, since every save carries the whole config. Both halves are held by
`config::tests::saves_at_once_all_land_and_the_newest_is_the_one_kept` and
`config::tests::a_save_overtaken_by_a_newer_one_does_not_replace_it`, each watched failing on the
code it was written for.

That fix did not hold when two saves passed the check at once. The check of the number was a
compare and swap and the rename came after it, with nothing holding the two together: an older
save passed the check, a newer one passed it and renamed, and the older renamed last, putting
back what the newer one had replaced. A test that runs the saves one after the other cannot see
it. Case: `saves_at_once_all_land_and_the_newest_is_the_one_kept` failed four times in CI on
2026-11-20, three on Windows and one on macOS, keeping 1006 or 1001 in place of 1007, and passed
on the commit tagged v0.5.0, which therefore ships the defect. On Linux the same test failed 12
times in 300. The check and the rename now sit under one lock, the writing of the temporary file
staying outside it; `config::tests::an_older_save_past_its_check_still_cannot_land_after_a_newer_one`
holds the older save between its check and its rename, failed three times out of three on the
old code, and the stress test then failed 0 times in 300.

## Typefaces

A bundled typeface is not a drop in replacement at the same nominal size. Space Grotesk is about a tenth wider than Ubuntu-Light at 11 px, 205.9 px against 190.0 for the longest line the map draws, while Space Mono matches Hack within 2 percent. Every fixed width tuned to the pixel against the old font has to be measured again: a node box whose longest line fitted with 2 px to spare no longer fits it, and a force layout given wider boxes flattens in a small canvas. Measure the strings that sit in fixed boxes before changing a font, not after.
Case: the map's node line elided and the largest gap around a centre going from 134.2 to 170.6 degrees at 1100 by 700, 2026-11-17.

## Release acceptance

A count written into a check goes stale when what it counts changes elsewhere. `ci/accept-artifact.sh` required four font licenses in the bundle, the four of `epaint_default_fonts`, written on 2026-11-14; Space Grotesk and Space Mono were bundled on 2026-11-17 with their own two, and nothing ran the script against a release in between. The count now comes from the tag's `licenses/fonts`.
Case: the run of Accept artifact on v0.5.0 stopping at "6 font licenses in the bundle, not four", the bundle and its six texts being right, 2026-11-20.

## macOS signing

The signing job's errors name the wrong culprit, and each one hides the next. `security import` answers "The user name or passphrase you entered is not correct" for a `.p12` and password that do not match, though the same file opens with its password on the Mac and `openssl` reads it (OpenSSL 3 still fails on the legacy RC2-40 cipher of a Keychain export without `-legacy`, which is no fault of the file). `notarytool` answers 401 "Invalid credentials" for an account password or a revoked app specific password, and once that is right, 403 "A required agreement is missing or has expired" for a developer program agreement not accepted. None of the three is a fault of the workflow. Prove each secret on the Mac before setting it, with the command the runner runs, and set the secret from the same shell variable so what was proved is what is stored: `security import` into a scratch keychain then `security find-identity -v -p codesigning`, and `xcrun notarytool history` with the Apple ID, the team ID and the app specific password.
Case: the Release of v0.6.0, 2026-11-23, failing four times in the job `macos-sign`: twice at the import, once with a 401 and, the credentials fixed, the agreement still to accept, then passing at its fifth attempt, notarisation Accepted, `spctl` reporting "source=Notarized Developer ID". A fifth failure on the way was no failure: the job was cancelled after 15 minutes in the macOS queue without starting, while the fork of libossia filled the account's five macOS slots.

## Auditing before a run

A glob of test files does not reach the harness they share. Asked which test runs open a socket, `grep -rln -E 'TcpListener|UdpSocket' crates/*/tests/*.rs` named `http.rs` and `service.rs` and left `websocket.rs` looking harmless, while both the listener and the UDP socket live in `crates/oscq-core/tests/common/mod.rs`, which every test that starts a synthetic server pulls in with `mod common;`. A glob of files does not descend into `tests/common/`. Ask the whole tree, `grep -rn` over `tests/`. And an audit of what may run while another measurement holds the machine has two axes, not one: a `cargo build` opens no socket at all and is the heaviest thing available here, so an answer about sockets does not answer the question.
Case: the audit of what could run while a bench on the same Mac was measuring `_oscjson._tcp`, 2026-11-24. `tests/common/mod.rs:257` binds the `TcpListener` and `:458` the `UdpSocket`, `websocket.rs:6` declares `mod common;`, and the first glob had reported none of the three.

On macOS `df -h /` reports the sealed system snapshot, not the volume the work is on. `/` answered 14 GiB used, 26 GiB available, 35 percent capacity, which reads as a volume with room, while `/dev/disk3s5` on `/System/Volumes/Data`, where the project and its caches live, answered 856 GiB used, 26 GiB available, 98 percent. Read `/System/Volumes/Data`, never `/`. And a figure about the disk is worth no more than the command that produced it: a `target/` said to reach 16 GB, measured by nobody, was 4.8 GiB when `du -sh` was finally run on it, by which time the first figure had been relayed twice and a plan built around the space it left.
Case: the human's Mac, 2026-11-24. `df -h / /System/Volumes/Data` prints the two lines together, `diskutil info /` reports `Volume Read-Only: Yes` with a container free space of 27.8 GB, and `du -sh` gives 4.8G for the clone's `target/`, 4.9G for the whole checkout.

A released bench lock is not a quiet machine. A peer session saying the bench is free means its own work has stopped, not that the real binary tests can run: the machine still carries whatever the person at it is doing, and the idle measurements of PRD 10 are refused by their pointer and their keys, as the entry above on real binary tests says. Measuring that costs well under a second and is cheaper than a refused ten second window and a number withdrawn afterwards: `ioreg -c IOHIDSystem | awk '/HIDIdleTime/ {print $NF/1000000000}'` gives the seconds since the last keyboard or mouse input, and `sysctl -n vm.loadavg` the load.
Case: a lock released at 13:54 on 2026-11-25 with the words "the machine is free", measured four minutes later as HID idle 0.0 s, a one minute load average of 76.83 and then 52.29, and Ableton Live 12 Suite running at pid 48725, so `real_binary_budgets` was not run and 0.7.0 went to its branch with that budget unmeasured rather than with an invalid number. In the same audit `pgrep -fl 'Max.app|REAPER|Live.app'` listed its own shell among the matches, the pattern being in its command line: the `pkill -f` trap of this file as a false positive rather than a dead shell, so `pgrep -x` or `ps -A -o comm=` for that question.

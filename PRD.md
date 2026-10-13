# Void OSCQ: Product Requirements Document

Status: v1 spec, locked for implementation. Amended on 2026-10-09 with the human's rulings on the M0 protocol discrepancies, see `docs/protocol-notes.md` section 10. Amended on 2026-10-12 with the human's rulings after M5: "cannot send" in place of "read only" (sections 3, 7.2, 9.6), the user's OSC port (7.2, 9.6, 9.7). Amended on 2026-10-13 for milestone M6: visual system, server list, pinned nodes, presets, traffic direction filter (sections 3, 9, 14, 17). Amended again on 2026-10-13, after the human found Refresh off screen: the toolbar and the narrow window rule (9.1). Then the narrow tree, the pane handles, the discovery line and the about box (9.1).
Audience: the engineer (human or agent) implementing the app from scratch.
Name: `void-oscq`, final. Binary `void-oscq`, app display name "Void OSCQ", repository `gluon/void-oscq`, public since 2026-10-12. The crate name `void-oscq` was free on crates.io at the time of writing; verify again before any publication.

Read this whole document before writing code. Everything marked LOCKED is a decision, not a suggestion. Everything marked VERIFY must be checked against a primary source before you rely on it, and the result written into `docs/protocol-notes.md`.

---

## 1. Problem

OSCQuery lets an application publish its OSC address space as JSON over HTTP, so other tools can discover and drive it with no manual setup. The reference client, Vidvox OSCQuery Browser, is macOS only, Cocoa, and deliberately one way: it sends OSC and never observes the remote state. Its tree widget also degrades on large address spaces, which is exactly what the interesting servers (VDMX, ossia, large Max patches) publish.

There is no portable, fast client that both browses an address space and lets you drive it while seeing the truth of what the remote side is doing.

## 2. Goals

1. One native app, same behaviour on macOS, Windows and Linux.
2. Discover OSCQuery servers on the LAN, and accept a manual host and port.
3. Render the full address space, including trees of several thousand nodes, with no visible lag.
4. Show each node's real current value, and keep it live when the server supports the WebSocket part of the protocol.
5. Let the user edit values with a widget appropriate to the declared type and range, and send the resulting OSC immediately.
6. Show raw JSON and a timestamped OSC traffic log, for debugging other people's implementations.
7. Ship as a single self contained binary per platform, started from CI.

## 3. Non goals for v1

Do not build these. They are listed so that the temptation is answered once.

- No layout editor, no user designed control surfaces, no widget scripting. Open Stage Control already exists and does that well. This app renders the server's namespace, it does not invent a UI on top of it.
- No MIDI of any kind.
- No OSCQuery server mode. The app is a client. Publishing an address space is what Vidvox OSCQuery Helper does.
- No mobile or tablet build, no web UI, no remote control of this app by another app.
- No theming system beyond one dark theme and one light theme, following the OS setting by default, either one forceable.
- No session persistence beyond the small set described in section 9.7. Pinned nodes (9.8) and the sixteen preset slots (9.9) are in scope since M6; nothing beyond them.
- No hiding of read only nodes (kept in reserve), no widget layout editor, no mapping or controller building.
- No plugin system, no embedded scripting language, no automation or sequencing.
- No code signing or notarisation in v1. The CI produces unsigned artifacts.
- No OSC sending to a server whose `HOST_INFO` declares `OSC_TRANSPORT` `TCP`. The proposal defines no OSC over TCP framing. Its nodes keep the access the server declares; the writable ones say "cannot send", with the reason displayed. They are never shown as read only.

Anything that smells like "while we are here, we could also" is out of scope until v1 ships and is used in anger.

## 4. Reference material

- OSCQuery proposal and specification: https://github.com/Vidvox/OSCQueryProposal. This is the normative source. Fetch it in M0 and keep a copy of the relevant parts in `docs/protocol-notes.md`.
- Vidvox OSCQuery Browser documentation, the app this replaces and must at least equal: https://docs.vidvox.net/oscquery-browser/
- Vidvox OSCQuery Helper, used as a test server: https://docs.vidvox.net/oscquery-helper/
- VVOSCQueryProtocol, the reference Objective C implementation, useful when the written spec is ambiguous: https://github.com/Vidvox/VVOSCQueryProtocol
- ossia, a second independent implementation to test interoperability against: https://ossia.io
- Open Stage Control, reference for control widget ergonomics only, explicitly not an architectural model: https://framagit.org/jean-emmanuel/open-stage-control

Precedence on protocol questions: the proposal wins. Where the proposal contradicts itself, VVOSCQueryProtocol wins. The answers, with sources, live in `docs/protocol-notes.md`.

## 5. Stack, LOCKED

| Concern | Choice | Why |
| --- | --- | --- |
| Language | Rust, stable toolchain, edition 2024 | One binary, no runtime to install, predictable latency, same code on three platforms |
| GUI | `eframe` / `egui` | Immediate mode, no webview, trivial cross compilation, built in virtualised list primitives |
| Async runtime | `tokio`, multi thread, confined to the network layer | HTTP, WebSocket and timers in one place |
| HTTP client | `reqwest` with `rustls` TLS backend | No OpenSSL dependency on any platform. OSCQuery is plain HTTP but the TLS free build must still link cleanly |
| WebSocket | `tokio-tungstenite` with `rustls` | Text frames for commands, binary frames for OSC packets |
| OSC encode and decode | `rosc` | Mature, no unsafe, covers bundles and all common type tags |
| mDNS and DNS-SD | `mdns-sd`, pure Rust | Identical on all three platforms, no Avahi or Bonjour SDK to link, no system daemon requirement |
| JSON | `serde` and `serde_json` | Address space documents are deserialised into typed structs, never walked as untyped values in UI code |
| Logging | `tracing` with `tracing-subscriber`, env filter | Needed to debug other people's servers |
| Error type | `thiserror` in library crates, `anyhow` at the binary boundary | Library errors are matched on, application errors are reported |
| Fuzzy matching | `nucleo-matcher` | Path search over thousands of nodes must stay interactive |
| Channels, both directions | `flume` | One channel crate for the whole project, see 6.2: non blocking drain on the UI thread, `recv_async` on the network side. The channel `mdns-sd` already returns, so discovery needs no bridge thread |
| URLs | `url` | Typed HTTP and WebSocket addresses, the same crate `reqwest` re-exports |

Do not add a dependency that is not in this table without recording the reason in `docs/decisions.md`. In particular: no `egui` widget mega crates, no ORM, no actor framework, no config framework. 

Pin exact versions at implementation time with `cargo add`, then commit `Cargo.lock`. The versions are not written here on purpose, because a stale version number in a PRD is worse than none.

Minimum supported platforms: macOS 12 on aarch64 and x86_64, Windows 10 on x86_64, Linux on x86_64 with glibc 2.31 or newer, X11 and Wayland.

## 6. Architecture, LOCKED

### 6.1 Crate layout

A Cargo workspace with three crates. This split exists so the protocol can be tested without a GUI, and so that a future headless tool can reuse it.

```
void-oscq/
  Cargo.toml              workspace
  crates/
    oscq-core/            protocol: types, HTTP client, WebSocket client, OSC send
    oscq-discovery/       mDNS browsing behind a trait, plus manual entry
    void-oscq/            the eframe application, UI only
  tests/
    fixtures/             captured JSON documents from real servers
  docs/
    protocol-notes.md
    decisions.md
```

`oscq-core` and `oscq-discovery` must not depend on `egui` or `eframe`. If a UI concern leaks into them, the split has failed.

`oscq-discovery` depends on `oscq-core` for shared types. Never the reverse. `void-oscq` depends on both.

### 6.2 Threading model

Three kinds of thread, with explicit boundaries.

1. The UI thread, owned by `eframe`. It never blocks, never performs I/O, never calls `block_on`. It owns the entire `AppState`.
2. One `tokio` multi thread runtime, spawned at startup, owning every HTTP request, every WebSocket connection, and the UDP socket used to send OSC.
3. The `mdns-sd` daemon, which manages its own thread and hands back a receiver.

Communication is message passing in both directions, no shared locks on application state:

- UI to network: `flume::Sender<Command>`, from an unbounded channel. The network side receives with `recv_async` inside the `tokio` runtime.
- Network to UI: `flume::Sender<Event>`, drained at the top of every frame in a non blocking loop with a bounded iteration count so a flood cannot starve rendering.
- Any event that mutates visible state calls `egui::Context::request_repaint()`.

When idle the app must not repaint. Continuous repaint is a bug, not a default.

### 6.3 Command and event vocabulary

This is the contract between the two halves. Extend it deliberately, do not bypass it.

```rust
enum Command {
    StartDiscovery,
    StopDiscovery,
    Connect { server_id: ServerId, http_base: Url },
    Disconnect { server_id: ServerId },
    RefreshTree { server_id: ServerId, from_path: OscPath },
    RefreshValue { server_id: ServerId, path: OscPath },
    Listen { server_id: ServerId, path: OscPath },
    Ignore { server_id: ServerId, path: OscPath },
    SendOsc { server_id: ServerId, path: OscPath, args: Vec<OscType> },
}

enum Event {
    ServerFound(DiscoveredServer),
    ServerChanged(DiscoveredServer),
    ServerStale(DiscoveredServer),
    HostInfo { server_id: ServerId, info: HostInfo },
    TreeLoaded { server_id: ServerId, root: Node },
    SubtreeLoaded { server_id: ServerId, path: OscPath, node: Node },
    ValueChanged { server_id: ServerId, path: OscPath, value: Vec<OscType> },
    PathAdded { server_id: ServerId, path: OscPath },
    PathRemoved { server_id: ServerId, path: OscPath },
    PathRenamed { server_id: ServerId, old: OscPath, new: OscPath },
    TransportState { server_id: ServerId, state: TransportState },
    TrafficLogged(TrafficRecord),
    Error { server_id: Option<ServerId>, error: UserFacingError },
}
```

`ServerId` is an opaque internal identifier, stable for the lifetime of a connection, never a user visible string and never the mDNS instance name.

The three server events mirror what `oscq-discovery` emits, `Found`, `Changed` and `Stale`, each with a full snapshot of the server. Discovery never removes a server: when its advertisement goes away, or a liveness check fails, the entry is marked stale through the `stale` flag it carries, kept so the user can retry, and revived with the same `ServerId` if the server is seen again.

## 7. Protocol implementation

VERIFY every attribute spelling, every query parameter and every WebSocket command name against the proposal repository in M0 before writing the deserialiser. The list below is the implementation target. It was amended after M0 to follow the proposal, and VVOSCQueryProtocol where the proposal contradicts itself. Any further disagreement found must be written into `docs/protocol-notes.md`.

### 7.1 Address space document

A GET on a path returns a JSON object describing that node. Expected keys:

- `FULL_PATH`: the OSC path of this node. Required.
- `CONTENTS`: object mapping child name to child node object. Present on containers. A node without `CONTENTS` is a method.
- `TYPE`: OSC type tag string, for example `f`, `ff`, `ffff`, `s`, `T`, `i[ff]i`. Present on methods that take or hold a value, may be omitted on a method that takes none.
- `VALUE`: current values, one entry per argument, see per argument attributes below.
- `RANGE`: one entry per argument, each optionally carrying `MIN`, `MAX`, `VALS`.
- `ACCESS`: bit mask. 0 no value, 1 read only, 2 write only, 3 read and write.
- `DESCRIPTION`, `TAGS`, `EXTENDED_TYPE`, `UNIT`, `CRITICAL`, `CLIPMODE`, `OVERLOADS`: optional, display or behaviour hints.

Per argument attributes are `VALUE`, `RANGE`, `EXTENDED_TYPE`, `UNIT` and `CLIPMODE`. Each accepts every shape the proposal allows, and each shape is covered by a fixture:

- a JSON array with one entry per top level argument of `TYPE`;
- nested arrays, where `TYPE` groups arguments with `[` and `]`, for example `TYPE` `i[ff]i` with `VALUE` `[100, [1.0, 1.0], 100]`;
- `null` in place of any entry that has no value for that attribute;
- a single non array value standing for every entry, allowed when all entries are equal.

Deserialise into a typed `Node` struct with `Option` fields. Unknown keys are preserved verbatim in a `serde_json::Map` so the raw JSON view stays faithful and so forward compatibility costs nothing. Never panic on a missing optional attribute. A server that omits `ACCESS` is common: write is then allowed, and read is allowed only if `VALUE` is listed as `true` in `HOST_INFO` `EXTENSIONS`.

Implementation requirements:

- Parse `TYPE` into a tree of argument types, including the OSC tags that carry no payload (`T`, `F`, `N`, `I`) and the `[` `]` array groups. A node without `CONTENTS` is a method, even if it has no `TYPE`.
- `r` is a colour on its own, its `VALUE` is a `#RRGGBBAA` string. `EXTENDED_TYPE` colour names are accepted in both spellings found in the proposal, `color.rgb.*` and `color.rgba.*`.
- `CLIPMODE` is descriptive: it says how the server clips. It is displayed, never acted upon.
- `VALUE` length may disagree with `TYPE` length in the wild. Render what is there, log a warning once per path, do not drop the node.
- Treat the document as untrusted input. A malformed or hostile JSON body must produce an error event, never a panic and never an unbounded allocation. Cap the response body at 64 MB.

### 7.2 Host info

A GET with the `HOST_INFO` query parameter returns the server's transport details: a display name, the OSC IP and port, the OSC transport (UDP or TCP), the WebSocket IP and port when supported, and an `EXTENSIONS` object advertising which optional features exist.

The path is irrelevant to the proposal, the app queries the root.

The OSC destination for every message sent by this app comes from `HOST_INFO`, not from the mDNS record. Getting this wrong is the single most common bug in OSCQuery clients: the HTTP server and the OSC receiver are frequently on different ports.

Defaults defined by the proposal, applied when a key is absent:

- `OSC_IP` absent: the HTTP host.
- `OSC_PORT` absent: the HTTP port. The inference can be wrong, the server having no way to say so: Max 9 serves OSCQuery on 30339 and receives OSC on 30338 (`docs/protocol-notes.md` section 12). The user can then set the OSC port for that server; it is kept in the config (9.7), and no later inference replaces it. A port advertised in `HOST_INFO` always wins over a port the user set earlier: the user's port fills a silence of the server, it does not contradict a declaration. The UI shows whether the OSC port was advertised in `HOST_INFO`, inferred from the HTTP port, or set by the user.
- `OSC_TRANSPORT` absent: `UDP`. When it is `TCP`, nothing is sent in v1: the affected nodes keep their declared access, and the writable ones say "cannot send" and why.
- `WS_IP` and `WS_PORT` absent: the HTTP host and port, see 7.4.

If the `HOST_INFO` request itself fails, the server is browsable but not controllable. Say so in the UI and disable the editing widgets.

### 7.3 Partial queries

A GET on a sub path with an attribute query parameter returns just that attribute, for example `/layer1/opacity?VALUE` returns `{"VALUE": [0.82]}`. Use this for single value refresh. Never re-fetch the whole tree to refresh one value.

HTTP 200 and 204 are both success. 200 with an empty object means the attribute is unsupported or absent for that node. 204 means the request is valid but no content is available, for example `VALUE` on a node that is not readable. Neither is an error.

### 7.4 WebSocket

The proposal has no flag advertising WebSocket support. When `HOST_INFO` gives `WS_IP` or `WS_PORT`, open one connection per server to that address and port. When it gives neither, attempt one connection on the HTTP host and port. If that single attempt fails, fall back silently to refresh only mode: no error is raised, and the status bar shows refresh only as required by 9.6.

- Text frames carry JSON commands. Client to server: a `COMMAND` of `LISTEN` or `IGNORE` with `DATA` set to the path. Server to client: `PATH_CHANGED`, `PATH_ADDED`, `PATH_REMOVED` with `DATA` set to the path, and `PATH_RENAMED` with `DATA` set to an object `{"OLD": path, "NEW": path}`.
- Binary frames carry raw OSC packets, one packet per frame. Decode incoming ones with `rosc` and treat them as value updates for the matching path. Value updates arrive this way, after `LISTEN`, and in no other way.
- `PATH_CHANGED` signals a structure change, not a value change. Reload the subtree at the received path, including all its descendants. Servers may coalesce several changes into one `PATH_CHANGED` on a common ancestor.
- On `PATH_ADDED` or `PATH_REMOVED`, re-fetch the parent container's subtree, not the whole tree.
- On `PATH_RENAMED`, move local state, including active listens, from `OLD` to `NEW`.
- Reconnect with exponential backoff starting at 500 ms, capped at 15 s, with jitter. Re-issue every active `LISTEN` after a reconnect. Show the transport state in the UI.

LISTEN discipline matters. `LISTEN` is not recursive: listening to `/foo` does not deliver `/foo/bar`. Send one `LISTEN` per visible or pinned method, none for containers, and send `IGNORE` when the method leaves. A client that listens to three thousand nodes is a client that nobody will run twice.

If the server has no WebSocket, the app still works: values are fetched on expand, on explicit refresh, and after each send. The difference must be visible in the UI, not silent.

### 7.5 Sending OSC

Default transport UDP, destination taken from `HOST_INFO`. Encode with `rosc`, respecting the node's declared `TYPE`. Coerce the user's input to the declared types before sending, never send a float where the node declared an int.

`T` and `F` are interchangeable on send, as the proposal allows.

Widgets clamp on `RANGE` only, and show the clamped value. `CLIPMODE` is displayed, never acted upon.

UDP is the default transport for sending. OSC over the WebSocket is opt in, per server, from the server panel, because the proposal does not require servers to accept OSC from clients over the socket. The active transport is always visible.

Every sent and received message is appended to the traffic log, with a monotonic timestamp, direction, path and arguments.

## 8. Discovery

### 8.1 Strategy, LOCKED

Browse for `_oscjson._tcp.local.` with `mdns-sd`. VERIFY the service type string against the proposal in M0. `mdns-sd` requires the `.local.` suffix, the proposal writes `_oscjson._tcp.`. Also browse `_osc._udp.local.`, a product choice and not a requirement of the proposal, and show those as "OSC only, no query support", greyed out and not connectable, because knowing that a device is there but silent is useful information.

Behind a trait:

```rust
pub trait Discovery: Send {
    fn start(&mut self) -> Result<flume::Receiver<DiscoveryEvent>>;
    fn stop(&mut self);
}
```

One implementation in v1, `MdnsSdDiscovery`. The trait exists so that a native backend can be added later if a platform misbehaves, not because a second backend is planned.

### 8.2 Practical requirements

These are the things that make mDNS work in a real venue, and each one must be handled:

- Multiple network interfaces. Resolve every advertised address, prefer IPv4, skip loopback and the common Docker bridge range, and keep the others as fallbacks rather than discarding them.
- Services that vanish without a goodbye packet. Mark a server stale rather than deleting it instantly, and let the user retry.
- Duplicate advertisements for one logical server across interfaces. Deduplicate by host and port, not by instance name.
- Manual entry of `host:port`, always available, with the last ten entries remembered. This is the escape hatch for VPNs, routed subnets and firewalled labs, and it must never be buried in a menu.
- On Windows, service removal events may not arrive. Fall back to a periodic liveness check of known servers.
- Never block the UI on discovery. An empty list after two seconds shows a hint, not a spinner forever.

## 9. User interface

One window. No modal dialogs except the manual connection entry and the about box.

### 9.1 Layout

```
+--------------+---------------------------------------------+-------------+
| DISCOVERED   | Tree JSON Traffic  Refresh  [filter /] [root] | PINNED      |
|  . Helper    +---------------------------------------------+  master 0.5 |
| MANUAL       | name            | control           |  meta |  opacity    |
|  . Max     x | - composition   |                   |       |             |
|              | | master        | [====|====] 0.500 | f  rw | PRESETS     |
| [+ manual]   | | clear         | [ send ]          | I   w | 01 02 03 04 |
|              | - layer1        |                   |       | 05 06 07 08 |
| OSC PORT     | | opacity       | [=======|=] 0.820 | f  rw | ...         |
|  30338 [Set] |                                             |             |
+--------------+---------------------------------------------+-------------+
| state | server | transport | nodes | message                              |
+--------------------------------------------------------------------------+
```

Left, the server pane: discovered servers, then manual entries, two groups apart, each server with a status dot (discovered, connecting, connected, websocket live, stale, error). A manual entry has a remove button; a discovered one has none, since it comes back. Removing a manual entry drops its saved OSC port. Below the list, the OSC destination of the selected server and, when its port is not advertised, the OSC port field (7.2). With no server found, it says since when it has been searching, as a time of day: a running count would repaint the window at rest. Then `+ manual`, the theme and the about box: the name, the version, Julien Bayle, Structure Void, the license, and a link to the repository, sober, imitating no system window. On macOS the application menu's About item opens the same box. Multiple servers may be connected at once; the centre shows the selected one.

Centre: a toolbar over the address space: three tabs and no more, Refresh, then the filter and the root box sharing what the line has left. With a server that never notifies, Refresh is the only way to read a value again. On a wide window the side panes keep their width, the centre takes the rest and the toolbar holds on one line; the two fields go to a line of their own, together, only when the line leaves them less than their least widths.

Right: the pinned nodes (9.8) and the presets (9.9), visible whatever the tab and whatever the tree shows.

Bottom: the status bar, one line in fixed zones: connection state, server, transport (OSC destination, values live or on refresh only), node count, then the last error or note. Each zone is short; the details are on hover.

Narrow windows: whatever the window width, down to its 640 px minimum, and however the side panes were dragged, no control of the toolbar, the server pane, the right pane or the status bar leaves the screen. The toolbar wraps onto further lines; the server pane narrows first, down to 128 px, then the right pane, down to 288 px; the centre keeps at least 224 px; the status bar zones narrow together, their text cut and whole on hover. On a narrow tree the name gives way first, then the metadata, down to the access flags alone, then the name again: the widget is the tool and keeps its width, the name gives its full path on hover. A pane's handle stops dead at its limits, and the pane follows it whole.

Visual system, applied everywhere:

- A 4 px base grid. Every size, margin and gap is a multiple of 4 px: rows 24 px, tree indentation 16 px per level, gaps between groups 16 px, inside a group 4 or 8 px.
- One type scale: 11, 13 and 15 px for the interface, 12 px monospace for data. Data, meaning paths, values, type tags, access flags, traffic and JSON, is monospace; the interface around it is proportional.
- Groups are told apart by space, not by rules. Elements that belong together sit close: a node's name is next to its control, not at the far side of the window.
- The theme follows the OS by default; dark or light can be forced; the choice is kept (9.7).

### 9.2 Tree tab

- Virtualised rendering. Only visible rows are laid out, using `egui`'s row range primitives. Expanding a container with five thousand children must not stall a frame.
- Lazy loading. Fetch a subtree when its container is first expanded, not at connection time, unless the whole document arrived in the root GET, which is the common case. Handle both.
- Per row, three strict columns. Name: the leaf name, indented 16 px per level, with thin vertical guides for each ancestor level. Control: the editing widget, which carries the value; a node with no widget shows its value as text in the same column; every control starts on one vertical line whatever its type. Metadata, right aligned and quiet: a "sent" mark while a sent value is unconfirmed (9.6), the type tags and the access flags. One value per row: no value text beside a widget. Full path on hover.
- Right click on a row: copy its full path, pin or unpin it (9.8).
- A path prefix box at the top of the pane that reroots the display at any container, matching the reference app's behaviour.
- Fuzzy filter over full paths, matching live, with matched characters highlighted. Filtering must never trigger a network request. A hit shows its name first, then the path of its container cut from the left when the column is short, so that the nearest containers, which tell apart hits of one name, stay in sight; the full path on hover.
- Keyboard: arrows to move, right and left to expand and collapse, enter to focus the widget, escape to leave it, `/` to focus the filter, `cmd/ctrl+c` to copy the full path.

### 9.3 Editing widgets by type

Chosen from `TYPE`, `RANGE` and `EXTENDED_TYPE`:

| Declared | Widget |
| --- | --- |
| `f` or `d` with MIN and MAX | horizontal slider, with a numeric field for exact entry |
| `f` or `d` without range | drag value, fine with modifier held |
| `i` or `h` with MIN and MAX | integer slider |
| `i` with `VALS` | dropdown |
| `s` with `VALS` | dropdown |
| `s` free | single line text field, commit on enter |
| `T` and `F` | toggle |
| `I` (impulse) or `N` (nil) | momentary button |
| `r`, without any `EXTENDED_TYPE` | colour picker |
| `ffff` named as a colour by `EXTENDED_TYPE`, `color.rgb.*` or `color.rgba.*` | colour picker |
| `ff` or `fff` | grouped drag values on one row |
| anything else | read only display of the value, plus a raw argument entry field |

The widget carries the value: no separate value readout beside it. Write only nodes show the widget holding the last value sent, marked "sent" in the metadata column. Read only nodes show the value with no widget. Never show an editable widget that cannot send. Widgets clamp on `RANGE` only, never on `CLIPMODE`.

Sending policy: continuous widgets send while dragging, rate limited to one message per 8 ms per path, with the final value always sent on release. Text and numeric fields send on commit.

### 9.4 JSON tab

The raw document for the currently selected subtree, syntax coloured, with a copy button and a save to file button. This is a debugging tool for people implementing servers, so it shows exactly what arrived, not a re-serialisation of the parsed model.

### 9.5 Traffic tab

A timestamped log of OSC in and out for the selected server. Columns: time, direction, path, type tags, arguments. A filter box on text, a direction filter (all, in, out), a pause button, a clear button, and an export to text file button. Bounded ring buffer of 10000 entries, oldest dropped. A preset recall (9.9) is one entry, a block naming the slot and the number of messages, which opens to show each message.

### 9.6 Feedback and honesty

The UI must never imply state it has not observed. Specifically:

- A value that was sent but not confirmed by the server is drawn differently from a value the server reported.
- A server without WebSocket support shows "values on refresh only" in the status bar. No silent polling.
- The OSC destination shows whether its port was advertised in `HOST_INFO`, inferred from the HTTP port, or set by the user.
- A node keeps the access its server declares, whatever the connection. A writable node nothing can be sent to, because of `OSC_TRANSPORT` `TCP`, a connection that is not up or any other reason, says "cannot send", with the reason. "read only" on a writable node misstates its access and is never shown.
- Errors appear in a status line with the failing path and the reason, not as a generic toast.

### 9.7 Persistence

A single config file in the OS standard config directory, written atomically on change. It contains: window size and position, manual server entries, last selected server, theme preference, pinned paths per server (`pinned_paths`, 9.8), the OSC ports set by the user per server (`osc_ports`, 7.2), and the sixteen preset slots (`presets`, 9.9). Nothing else. No database.

### 9.8 Pinned nodes

The working surface. A node is pinned or unpinned from the tree, by right click, and unpinned from the panel, by the button at the end of its row or by right click. Pinned nodes of the selected server are listed in a panel of their own, with the same name, control and metadata columns as the tree, the name being the full path, and stay visible whatever the tree shows: collapsed containers, a filter, another root, another tab. The set is kept per server `host:port` in the config. Pinned methods are listened to (7.4) as visible rows are. A pinned path the server does not publish is listed as absent, not dropped.

### 9.9 Presets

Sixteen slots, for debugging, not for performance: one click captures, one click recalls, no confirmation. Each slot names its two actions, **Recall** and **Capture**; a slot holding values is told from an empty one at a glance, on a tinted ground, its number plain, Recall on; an empty slot has a faint number and Recall off.

- Capture takes every writable method of the loaded tree of the selected server or, when nodes of that server are pinned, only the pinned ones. No selection interface: the pinned set states the scope. A method's value is the value sent and not yet confirmed if there is one, else the value read; a method with neither, or with an impulse, nil or array argument, is skipped, and the count says so.
- A slot records the server it came from (`host:port` and name), each method's path, type tags and arguments.
- Recall sends every captured value to the selected server at once, as separate messages to the OSC destination (7.5), shown in the traffic log as one block (9.5). Against another server, or a server whose tree changed, it sends what matches, a writable method at the same path with the same type tags, and reports the rest as a short count. It never blocks.
- Slots are kept in the config (9.7).

## 10. Performance budgets

These are requirements, and M4 verifies them with a committed benchmark against the synthetic server.

| Scenario | Budget |
| --- | --- |
| Cold start to window visible | under 300 ms |
| Parse and model a 5000 node document | under 150 ms |
| Frame time while scrolling that tree | under 8 ms at p95 |
| Idle CPU, connected, 50 active listens | under 1 percent of one core |
| Idle repaints, no input reaching the window | zero |
| Slider drag to UDP packet leaving the socket | under 2 ms |
| Resident memory with 5000 nodes connected | under 150 MB |
| Release binary size, macOS aarch64 | under 25 MB |

If a budget cannot be met, record the measured number and the reason in `docs/decisions.md`. Do not quietly relax the table.

Idle means no input reaching the window. Each input event that does reach it costs at least 3 frames in `egui`: the frame of the event, the repaint `egui` asks for after any input, and the second pass it runs after every immediate request. That floor belongs to `egui`; it is documented, not worked around. Amended on 2026-10-12 by the human's ruling.

Closed on 2026-10-12: every budget is measured and met, with the final numbers and how each was taken in `docs/decisions.md`.

The `eframe` renderer stays `wgpu`. If the binary size budget fails at M5, switch to `glow` then, not before.

## 11. Error handling

Every failure has a user facing message naming what failed and what the user can do. No `unwrap` outside tests. No panic path reachable from network input.

Cases that must be handled explicitly, each with a test:

- HTTP refused, timed out, or returned a status other than 200 and 204.
- Attribute query answered with 204, or with 200 and an empty object: not an error, the attribute is shown as unavailable.
- `HOST_INFO` with `OSC_TRANSPORT` `TCP`.
- Response body that is not JSON, or is JSON but not an address space.
- `HOST_INFO` missing, or lacking an OSC port (inferred from the HTTP port).
- WebSocket advertised but refusing connections, or dropping mid session.
- Node whose `TYPE` and `VALUE` disagree in length.
- Per argument attributes as nested arrays, with `null` entries, or as a single scalar.
- Node without `CONTENTS` and without `TYPE`.
- Path containing characters that are legal in JSON but not in OSC.
- Server disappearing from mDNS while connected.
- Two servers advertising the same instance name.
- IPv6 only advertisement.
- A document deep enough to blow a recursive parser. Parse iteratively or cap depth at 64 and report.

## 12. Testing and proof

Three layers, all runnable by `cargo test` except where noted.

1. Unit tests in `oscq-core` over committed JSON fixtures in `tests/fixtures/`. Capture real documents from VDMX, OSCQuery Helper and ossia and commit them. Hand written fixtures alone are not acceptable: real servers deviate from the spec and that is the point of this layer.
2. Integration tests against a synthetic OSCQuery server implemented inside the test suite, covering: HTTP only server, WebSocket server, server that drops the WebSocket mid session, 5000 node document, malformed document, and a server whose OSC port differs from its HTTP port.
3. Manual acceptance against real software, run by the human, scripted as a checklist in `docs/acceptance.md`. The agent cannot run this and must not claim it passed. The checklist covers Vidvox OSCQuery Helper, VDMX if present, and an ossia based patch.

A measurement from the synthetic server is not proof that the app works against real software. Report both separately, and say which is which.

## 13. Build, CI, distribution

- `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test` on every push, on all three platforms.
- GitHub Actions matrix: `macos-latest` building both aarch64 and x86_64 then joining them with `lipo` into a universal binary, `windows-latest` for x86_64 MSVC, `ubuntu-22.04` for x86_64 glibc.
- Artifacts on every tagged commit: a macOS `.app` inside a zip, unsigned; a Windows `.exe`, unsigned; a Linux binary and a `.desktop` file in a tarball.
- No notarisation and no signing in v1. Leave a clearly marked, disabled signing step in the macOS workflow with a comment pointing at the secrets it would need, so enabling it later is a configuration change, not a rewrite.
- README documents the unsigned first run procedure for macOS and Windows in plain language.
- Linux build dependencies are listed in the workflow and in the README. Do not assume a desktop environment is present in CI.

## 14. Milestones

Each milestone ends with a commit on its own branch, a passing CI run, and the stated verifiable condition met. Do not start the next one before the current condition is demonstrably true.

**M0, protocol groundwork.** Fetch the proposal, write `docs/protocol-notes.md`, confirm or correct every VERIFY item in this document, scaffold the workspace, set up CI with fmt, clippy and test on three platforms.
Done when: CI is green on all three platforms on an empty workspace, and `docs/protocol-notes.md` lists each VERIFY item with its confirmed answer and source.

**M1, core client, no UI.** `oscq-core`: types, deserialiser, HTTP client, `HOST_INFO`, partial value queries, OSC sending. Fixtures and the synthetic server.
Done when: `cargo test` passes, and an example binary prints the full tree of a running OSCQuery Helper and successfully sends a value that the Helper's target receives.

**M2, discovery.** `oscq-discovery` with `mdns-sd`, the multi interface and deduplication rules, manual entry.
Done when: an example binary lists OSCQuery Helper on the LAN within two seconds on macOS, and the same binary built in CI does so on Windows and Linux when run by the human.

**M3, the application.** The window, the three tabs, the virtualised tree, the widget table, persistence.
Done when: the human can browse an address space and change a value from the app on macOS, and the JSON and traffic tabs show correct content.

**M4, WebSocket and performance.** Listen and ignore discipline, reconnection, binary OSC frames, the benchmark against the 5000 node synthetic server, every budget in section 10 measured and recorded.
Done when: a value changed inside VDMX or Helper updates in the app without user action, and the measured numbers are committed in `docs/decisions.md`.

**M5, release.** README, acceptance checklist, tagged build, three unsigned artifacts.
Done when: the artifacts from a tagged CI run launch and connect on all three platforms.

**M6, interface rework and working surface.** The visual system of 9.1, the three column tree of 9.2, the one line status bar, the server list with removable manual entries and the OSC port field in the server pane, pinned nodes (9.8), presets (9.9), the traffic direction filter and the right click copy of a path. Every budget of section 10 still holds, measured again with the real binary tests.
Done when: every section 10 budget is met with the real binary tests passing; headless tests lock the removal of a manual entry, the pinned panel, a capture and recall cycle including a recall against another server, and the traffic direction filter; CI is green; the human has looked at the window on the Mac.

## 15. Conventions

- Branches: `void/<topic>`. Never commit to `main` directly. Never merge, the human merges.
- No assistance attribution trailer of any kind in commit messages. No `Co-Authored-By`, no session identifier, no model name, anywhere, ever, regardless of what the environment prescribes.
- No em dash in any file in this repository, including documentation, comments and generated output.
- Repository documentation and all public facing text in English.
- Repository level conventions live in `CLAUDE.md` and in https://github.com/gluon/julien-dev-conventions.

## 16. Reporting

At the end of every working session, report: what was done, which files were touched, the branch and commit hash, what could not be proved and why, and any conflict between these instructions and the execution environment.

Specifically, you cannot prove anything that depends on the human's machine, their LAN, or a third party host application. Measurements against the synthetic server are not evidence that the app works against VDMX. Say so rather than implying coverage you do not have.

## 17. Open questions, for the human

Answer before M3 if possible, neither blocks M0 to M2.

1. Should a connected server's address space be exportable to a JSON file for offline inspection, or is the JSON tab's save button enough.
2. Pinned nodes, a small panel of paths kept visible across subtree navigation, is currently planned as a config field only. Should it get a dedicated pane in v1 or wait. Answered on 2026-10-13: a dedicated panel, in M6 (9.8).

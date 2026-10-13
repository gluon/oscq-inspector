# Void OSCQ

Void OSCQ is a desktop application for macOS, Windows and Linux that connects to OSCQuery servers. OSCQuery is a protocol by which an application publishes its OSC address space, the list of everything it can be controlled with, as JSON over HTTP. Void OSCQ finds those servers on the local network, shows their address space as a tree, shows each value, keeps the values live when the server pushes them, and lets you change them: every change is sent as an OSC message.

It is meant for people who build or debug OSC setups: checking what a server really publishes, driving it by hand, and watching the messages that go out and come back.

Copyright (C) 2026 Julien Bayle, Structure Void. Void OSCQ is free software under the GNU General Public License, version 3 or any later version ([LICENSE](LICENSE)), with an additional permission for the fonts compiled into the application ([LICENSE-EXCEPTION](LICENSE-EXCEPTION)). The fonts keep their own licenses, in [licenses/fonts](licenses/fonts).

The product specification is [PRD.md](PRD.md).

## What it does

- **Finds servers.** OSCQuery servers advertised on the network (Bonjour, `_oscjson._tcp`) appear on the left. Devices that only speak OSC, with no OSCQuery (`_osc._udp`), are listed greyed out, so you know they are there. A server that does not advertise, or sits behind a VPN or a router, is added by hand with `+ manual` and its `host:port`; the last ten are remembered, each removable. Discovered and manual servers are listed apart.
- **Shows the address space.** A tree of every container and method, fast with thousands of nodes. A filter box finds paths by fuzzy matching, each hit named first, then its container; a prefix box roots the tree at any container.
- **Shows the values, and says where they come from.** Each method shows its value in its widget, as the server reported it. A value you sent but the server has not confirmed is marked "sent". The status bar says whether values are live, pushed by the server over its WebSocket, or on refresh only.
- **Edits values with a widget that fits the type.** Sliders for numbers with a range, drag values without one, toggles, buttons for impulses, dropdowns for lists of values, text fields, colour pickers. The OSC goes to the address and port the server gives in its `HOST_INFO`, coerced to the types it declares. When the server does not give its OSC port, the app infers it from the HTTP port, as the protocol says; when that is wrong, as with Max 9, you set the right port for that server and it is kept.
- **Keeps your controls at hand.** Pin nodes from the tree: they stay in the pinned panel whatever the tree shows, per server, across restarts.
- **Presets.** Sixteen slots: one click captures the values of every writable node, or of the pinned ones when there are any; one click sends them all back, to the same server or to another, where what does not exist is counted and skipped.
- **Shows what arrived.** The JSON tab shows the document of the selected node exactly as the server sent it. The Traffic tab logs every OSC message sent and every value received, with times, filters them by text and direction, and exports them as text.

## What it does not do

- It is a client only: it does not publish an address space of its own.
- No layout editor, no custom control surfaces, no scripting, no automation, no MIDI.
- No OSC over TCP: nothing is sent to a server declaring TCP, and its writable nodes say so, with the reason.
- No sending OSC over the WebSocket in this version; OSC goes over UDP.
- No code signing: the downloads are unsigned, so your system warns you on first launch (see below).

## Download and first launch

Releases are built by GitHub Actions on each version tag, workflow **Release**. Open the run of the tag, scroll to **Artifacts**, and download the one for your system. GitHub hands every artifact as a zip: unzip it first.

Every artifact carries the license texts: the GNU GPL, its additional permission and the licenses of the bundled fonts, in a `licenses` folder, inside `Void OSCQ.app/Contents/Resources` on macOS.

### macOS 12 or later, Apple Silicon or Intel

The artifact holds `void-oscq-VERSION-macos-universal.zip`. Unzip it to get **Void OSCQ.app**, and move it to your Applications folder.

The app is not signed by an identified developer, so macOS stops it the first time:

1. Double click **Void OSCQ**. macOS says it cannot verify the app. Close that dialog: **Done**, **OK** or **Cancel** depending on the macOS version, never **Move to Trash**.
2. Open **System Settings**, then **Privacy & Security**. Scroll down to **Security**: a line says Void OSCQ was blocked. Click **Open Anyway**, then confirm with your password or Touch ID.
3. Click **Open** in the last dialog. From now on the app opens normally.

On macOS 12 to 14 there is a shorter way: Control click the app, choose **Open**, then **Open** again.

The first time the app looks for servers, macOS asks whether Void OSCQ may find devices on your local network. Click **Allow**. If you clicked **Don't Allow**, turn it on later in **System Settings**, **Privacy & Security**, **Local Network**. A server on the same Mac, at `localhost`, works either way.

### Windows 10 or later, 64 bit

The artifact holds `void-oscq-VERSION-windows-x86_64.exe` and a `licenses` folder. The `.exe` is the whole application: no installer, put it where you like.

The program is not signed, so Windows SmartScreen stops it the first time:

1. Double click the `.exe`. A blue window says **Windows protected your PC**.
2. Click **More info**, then **Run anyway**.

If Windows Defender Firewall asks whether to allow Void OSCQ on networks, allow it on **private networks**: discovery and the values pushed by servers need it.

### Linux, x86_64

The artifact holds `void-oscq-VERSION-linux-x86_64.tar.gz`. Unpack it and run the binary:

```
tar -xzf void-oscq-0.2.1-linux-x86_64.tar.gz
cd void-oscq-0.2.1-linux-x86_64
./void-oscq
```

To have it in your application menu with its icon, copy the binary to `~/.local/bin`, the `.desktop` file to `~/.local/share/applications`, and `void-oscq.png` to `~/.local/share/icons/hicolor/256x256/apps`.

What the system needs to run it: X11 or Wayland, `libxkbcommon-x11-0` on X11, and a Vulkan or OpenGL driver (Mesa is enough). It needs glibc 2.35 or newer, as on Ubuntu 22.04 or later. The release build prints this requirement, measured on the binary.

## Using it

- Select a server on the left: the app connects, reads its `HOST_INFO` and its whole tree.
- **Max 9** serves OSCQuery on its own machine only and does not advertise it: use `+ manual` with `localhost:30339`. It receives OSC on another port, 30338 by default, which it does not tell: set it in the OSC port field of the server pane, see below.
- Keyboard: arrows to move, right and left to expand and collapse, Enter to focus the widget of the selected row, Escape to leave it, `/` for the filter, F5 to refresh the selection, Cmd or Ctrl and C to copy the selected path.
- The window: servers on the left, the tree in the centre, pinned nodes and presets on the right, the status bar below. Each tree row has three columns: the name, the widget holding the value, and on the right the type tags and access flags (`r`, `w`, `rw`).
- The status bar is one line for the selected server: its state (live, connected but silent when the WebSocket is open and the server has sent nothing yet, connected on refresh only, error), its name, the OSC destination, the node count, and the last error with the path it concerns. Hover a zone for its details.
- The theme follows the system unless you force light or dark, at the bottom of the server pane; the choice is kept.
- **Removing a manual entry.** Its **x** in the server list removes it, and forgets the OSC port set for it. A discovered server has no **x**: it would come back with the next announcement.
- **Pinning.** Right click a row: **Pin** puts it in the pinned panel, where its widget works as in the tree; **Unpin** removes it, and so does the **×** at the end of its row in the panel. **Copy path** copies its full path. Pins are kept per server; a pinned path the server no longer has is shown as absent.
- **Presets.** **Capture** on a slot stores the values of the writable nodes, the pinned ones only when nodes are pinned; **Recall** sends them back, all at once, logged as one block in the Traffic tab. Recalled on another server, only the nodes at the same path with the same type are sent; the status bar counts those not found. No confirmation either way: a capture replaces the slot.
- **Setting the OSC port.** When the server does not advertise its OSC port, the server pane shows "port inferred from the HTTP port" under **OSC destination**, and an OSC port field. Type the port the server receives OSC on, then **Set OSC port** or Enter: the server pane says "port set by the user", sends go there, and the port is kept for that server in the config, used again at every connection and after a restart. **Infer OSC port** goes back to the HTTP port and forgets the setting. A port the server advertises is not editable and always wins, even over a port set earlier: the setting fills a silence of the server, it never contradicts what the server declares.
- Settings are saved in your system's configuration folder, in `void-oscq/config.json`: window size and place, manual entries, the last selected server, the theme, pinned paths, the OSC ports you set, the preset slots. JSON and traffic exports go to `void-oscq/exports/` beside it.

## Build from source

Rust stable, then:

```
cargo run -p void-oscq --release
```

Linux build dependencies, as installed in CI:

```
sudo apt-get install -y libxkbcommon-dev libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev
```

Checks run on every push, on the three systems:

```
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

## For developers

### Workspace

| Crate | Role |
| --- | --- |
| `crates/oscq-core` | protocol: address space model, document parser, HTTP client, `HOST_INFO`, WebSocket client, OSC sending |
| `crates/oscq-discovery` | mDNS browsing and manual entry, depends on `oscq-core` |
| `crates/void-oscq` | the `eframe` application, UI only |

`oscq-core` and `oscq-discovery` never depend on `egui` or `eframe`; CI enforces it. Every crate takes the one workspace version, which a release tag must match.

Protocol answers with their sources are in [docs/protocol-notes.md](docs/protocol-notes.md), including what OSCQuery Helper and Max 9 really do. Decisions are in [docs/decisions.md](docs/decisions.md), traps met in [PITFALLS.md](PITFALLS.md), test fixtures and their provenance in [tests/fixtures/README.md](tests/fixtures/README.md), the release checklist in [docs/acceptance.md](docs/acceptance.md).

### Tools

The application icon is drawn in code, `crates/void-oscq/src/icon.rs`: the window icon, the macOS `.icns`, the Windows executable's icon and the Linux `.png` all come from it, and no image file is stored. The release packaging writes the image files with:

```
cargo run -p void-oscq --example icons -- OUT_DIR
```

`oscq_discover` lists OSCQuery and OSC servers live, with each address and the interface it was seen on:

```
cargo run -p oscq-discovery --example oscq_discover
```

`oscq_probe` prints the `HOST_INFO` summary and the full tree of a server, and can send one value, parsed against the node's `TYPE`:

```
cargo run -p oscq-core --example oscq_probe -- http://HOST:PORT
cargo run -p oscq-core --example oscq_probe -- http://HOST:PORT --send /path ARG
```

The synthetic server of the test suite, 5000 methods by default, HTTP and WebSocket on one port. With `--stream HZ` it changes the value of every method the app listens to, HZ times a second, each on its own phase:

```
cargo run -p oscq-core --example synthetic_server -- --port 5678 --stream 2
```

### Performance budgets

PRD 10, on a release build. In process, also run in CI:

```
cargo test --release --workspace -- --ignored budget_ --nocapture --test-threads=1
```

The real binary, window included, against the synthetic server; needs a display, Linux or macOS, and the pointer outside the window:

```
cargo test --release -p void-oscq --test budgets_binary -- --ignored --nocapture --test-threads=1
```

### Release

The workflow `.github/workflows/release.yml` builds the three artifacts on a tag `v*`, and on any push that changes the packaging. The scripts are in `packaging/`. A tag must equal the workspace version:

```
git tag v0.2.1
git push origin v0.2.1
```

## Version history

### 0.2.1

- **Filter**: each hit named first, then the path of its container, cut from the left when the column is short: hits of one name in different containers stay told apart.
- **Toolbar**: the tabs and Refresh first, then the filter and root fields sharing the rest of the line; on a window of 1000 points or more it holds on one line, and the side panes keep their width.
- **Unpinning from the pinned panel**: a button at the end of each row, besides the right click.
- **Presets**: each slot names its two actions, **Recall** and **Capture**; a slot holding values is tinted, its number plain, Recall on; an empty one has a faint number and Recall off.

### 0.2.0

- **Interface rework**: a 4 px grid and one type scale; a tree in three columns, name, widget, type tags and flags, with indentation guides; the widget holds the value; monospace for data; a one line status bar in fixed zones, details on hover.
- **Narrow windows**: down to the 640 px minimum, no control leaves the screen. The toolbar wraps, Refresh included; the side panes narrow in turn and their handles stop at their limits; a narrow tree gives up the name, then the metadata down to the access flags, never the widget.
- **Server list**: discovered and manual servers apart; a manual entry can be removed, with the OSC port set for it; the OSC destination and the OSC port field, still correctable when the server does not advertise its port, are in the server pane; with no server found, it says since when it has been searching.
- **Pinned nodes**, kept per server, visible whatever the tree shows.
- **Sixteen preset slots**, captured and recalled in one click, kept in the config; a recall on another server sends what matches and counts the rest.
- **Traffic**: filter by direction; a preset recall logged as one block; right click a row to copy its full path.
- **About box**: name, version, Julien Bayle, Structure Void, license, repository link, from the About button and, on macOS, from the application menu.
- **License**: GPL-3.0-or-later, with an additional permission for the fonts compiled into the application. Every artifact carries the license texts; the Windows artifact holds the `.exe` and a `licenses` folder.

### 0.1.1

- **Application icon** on the three systems: the macOS `.icns` in the bundle, the Windows executable's icon, the Linux `.png` named by the `.desktop` file, and the window and the Dock while the app runs.
- **Version in the macOS bundle**: both version keys of `Info.plist` carry the release version, checked against the tag by the release workflow.
- **OSC port you can set** per server when the server does not advertise one; the status bar tells advertised, inferred from the HTTP port, or set by the user. Needed for Max 9, which receives OSC on 30338 and serves OSCQuery on 30339.
- **Errors say why**: a failed connection shows the system's reason, such as "Connection refused", not only "error sending request".
- **Errors that no longer hold are cleared**: an error disappears once the same operation succeeds, and a failed connection is said once.
- **Access is the server's**: when the connection fails, nodes keep the access the server declared; writable nodes say "cannot send", with the reason, never "read only". The same for a server declaring OSC over TCP.
- **Release workflow**: `actions/upload-artifact` v7, no Node 20 deprecation warning.

### 0.1.0

- First release: discovery over Bonjour and manual entries, the address space as a tree, values read and sent with widgets fitting their types, values live over the WebSocket where the server pushes them, the JSON and Traffic tabs. Unsigned artifacts for macOS (universal), Windows and Linux.

## Status

- M0, protocol groundwork: done, merged.
- M1, core client: done, merged. Proven by the human on 2026-10-09: `oscq_probe` printed the Helper's 22 node tree and a value sent to `/test/my_float` reached the Helper's target.
- M2, discovery: done, merged. Proven on the human's LAN on 2026-10-09: one entry in 0.18 s, lo0 and en0 merged, LAN IPv4 promoted.
- M3, application: done, merged. Tested by the human on the Mac against OSCQuery Helper.
- M4, WebSocket and performance: done, merged. Every PRD 10 budget met on the human's Mac or the macOS CI machine, see docs/decisions.md. Values pushed by the synthetic server seen live on the Mac; Max 9 accepts the WebSocket and sends nothing, shown as open and silent.
- M5, release: done, merged, tag `v0.1.0`. The tagged run built the three artifacts; the macOS app was installed, opened and connected to Max 9 by the human.
- M5 polish: done, merged. The OSC port setting checked by the human on the Mac against Max 9: sends reach 30338 once set, kept across a restart. The loss and return of a server, the system's cause in the error and the declared access kept were checked there too.
- v0.1.1: done, merged, tag `v0.1.1`. The tagged run 38269010531 built the three artifacts; on the human's Mac the app installed, showed its icon and 0.1.1 in the Finder, connected to Max 9 with the OSC port read from the config, and its sends moved the fader.
- Cleanup: done, merged. A reload that leaves the rows on screen is locked to send no `LISTEN` or `IGNORE`; the PRD says the repository is public; docs/acceptance.md states the known limit of WebSocket interoperability.
- M6, interface rework, server list, pinned nodes, presets, traffic filter: done, merged. Checked by the human on the Mac: the three real binary tests pass, cold start 91.5 to 105.7 ms, memory 97.8 MB, no repaint at rest. Headless tests lock the removal of a manual entry with its OSC port, the pinned panel across tabs and per server, a capture and recall on the same server and on another, and the traffic direction filter. The look waits on the human, docs/acceptance.md section 5. After the human's review: the toolbar, Refresh included, on screen at any window width; the pane handles stop dead at their limits; the narrow tree keeps the widget and gives up the name, then the metadata; the discovery line says since when it searches; the about box names the author, Structure Void, the license and the repository. License GPL-3.0-or-later, with an additional permission for the bundled fonts, after the dependency audit recorded in docs/decisions.md; every release artifact carries the license texts.
- About fix: done, merged. On macOS the application menu's About item opened the system's panel, empty outside a bundle; it now opens the application's own about box, whose six pieces of information a test checks one by one on screen. Checked by the human on the Mac: the six shown by both paths, the menu and the button.
- v0.2.0: done, merged, tagged and released; the three artifacts complete with their license texts, installed on the human's Mac.
- v0.2.1: on branch `void/v0-2-1`. Filter hits named first, the toolbar on one line on a wide window, unpinning from the pinned panel, preset slots that name their actions and show what they hold. The three crates at 0.2.1. The tag is the human's.

Backlog:

- Acceptance against a real server that notifies over its WebSocket (VDMX, ossia): a value changed inside it updates in the app without user action. Neither OSCQuery Helper nor Max 9 notifies; the synthetic server is the only proof so far. The known limit and what lifts it are in docs/acceptance.md.
- Real captured documents from VDMX and ossia, PRD 12. OSCQuery Helper and Max 9 are captured.
- OSC sent over the WebSocket, opt in per server (PRD 7.5).

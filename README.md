# OSCQuery Inspector

OSCQuery Inspector is a desktop application for macOS, Windows and Linux that connects to OSCQuery servers. OSCQuery is a protocol by which an application publishes its OSC address space, the list of everything it can be controlled with, as JSON over HTTP. OSCQuery Inspector finds those servers on the local network, shows their address space as a tree, shows each value, and lets you change them: every change is sent as an OSC message. It keeps the values live when a server pushes them over its WebSocket, but none of the three servers it was tested with pushes its values: only the synthetic server of this repository does, so live updates are proven against it alone.

It is meant for people who build or debug OSC setups: checking what a server really publishes, driving it by hand, and watching the messages that go out and come back.

Formerly void-oscq, shown as Void OSCQ: the versions published under that name, 0.2.3 and earlier, keep it in their downloads, `void-oscq-VERSION-...`, which hold **Void OSCQ.app** on macOS. It is the same application under a new name, and the downloads named below are those of the versions after 0.2.3.

**Download**: the [Releases page](https://github.com/gluon/void-oscq/releases), one file per system, free, unsigned. See [Download and first launch](#download-and-first-launch) for opening an unsigned app the first time. Changes between versions are in [CHANGELOG.md](CHANGELOG.md).

## Tested with

| Server | What it does | What it does not do |
| --- | --- | --- |
| Vidvox [OSCQuery Helper](https://github.com/Vidvox/VVOSCQueryProtocol) | advertised over Bonjour, gives its OSC port; values sent reach its OSC target | declares every node write only, so no value can be read back; no `LISTEN` over its WebSocket, nothing pushed |
| Max 9, which serves OSCQuery natively | values read and sent, with the OSC port set by hand | listens on the same machine only, `localhost:30339`; no Bonjour; does not say its OSC port, 30338 by default; ignores the `HOST_INFO` query; accepts the WebSocket and never sends anything on it |
| The synthetic server of this repository | 5000 methods, values pushed over its WebSocket; with `--peers serve`, the peers of the map, see below | a test server, not a real application; it is never itself a client of another server |

Not tested: VDMX, ossia and any other server. None of the servers above but the synthetic one publishes its peers: the map and the listener badges are proven against it alone. Reports of how the app behaves with them are welcome, see [CONTRIBUTING.md](CONTRIBUTING.md).

Systems:

- **macOS**: tested by hand.
- **Linux**: built, tested and started in CI under Xvfb, a virtual display.
- **Windows**: built and tested without its window in CI; since 0.2.3, every release build unzips the packed executable and starts it, and fails unless it draws its first frame. Never tried by hand.

## What it does

- **Finds servers.** OSCQuery servers advertised on the network (Bonjour, `_oscjson._tcp`) appear on the left. Devices that only speak OSC, with no OSCQuery (`_osc._udp`), are listed greyed out, so you know they are there. A server that does not advertise, or sits behind a VPN or a router, is added by hand with `+ manual` and its `host:port`; the last ten are remembered, each removable. Discovered and manual servers are listed apart.
- **Shows the address space.** A tree of every container and method, fast with thousands of nodes. A filter box finds paths by fuzzy matching, each hit by its name, with its container when another hit has the same name elsewhere; a prefix box roots the tree at any container. **Writable** leaves out what cannot be written, such as the metadata Max publishes for each parameter.
- **Shows the values, and says where they come from.** Each method shows its value in its widget, as the server reported it. A value you sent but the server has not confirmed is marked "sent". The status bar says whether values are live, pushed by the server over its WebSocket, or on refresh only.
- **Edits values with a widget that fits the type.** Sliders for numbers with a range, drag values without one, toggles, buttons for impulses, dropdowns for lists of values, text fields, colour pickers. The OSC goes to the address and port the server gives in its `HOST_INFO`, coerced to the types it declares. When the server does not give its OSC port, the app infers it from the HTTP port, as the protocol says; when that is wrong, as with Max 9, you set the right port for that server and it is kept.
- **Keeps your controls at hand.** Pin nodes from the tree: they stay in the pinned panel whatever the tree shows, per server, across restarts.
- **Presets.** Sixteen slots: one click captures the values of every writable node, or of the pinned ones when there are any; one click sends them all back, to the same server or to another, where what does not exist is counted and skipped.
- **Shows what arrived.** The JSON tab shows the document of the selected node exactly as the server sent it. The Traffic tab logs every OSC message sent and every value received, with times, filters them by text and direction, and exports them as text.
- **Shows who is connected, where a server publishes it.** OSCQuery has no way to ask a server who is connected to it. Some servers implement an extension that adds one, `PEERS`, and declare it in their `HOST_INFO`. For those, the Map tab draws each server and each of its clients, and the tree marks every method some other client listens to, with their count. A server that does not declare the extension is drawn and said not to publish its peers: nothing is guessed for it.

## What it does not do

- It is a client only: it does not publish an address space of its own.
- No layout editor, no custom control surfaces, no scripting, no automation, no MIDI.
- No OSC over TCP: nothing is sent to a server declaring TCP, and its writable nodes say so, with the reason.
- No sending OSC over the WebSocket in this version; OSC goes over UDP.
- The map reads; it connects, disconnects and reroutes nothing. It is not live either: the peers are read every two seconds by default while the map is shown, and when it is not, they are not read.
- No code signing: the downloads are unsigned, so your system warns you on first launch (see below).

## Download and first launch

Each version is on the [Releases page](https://github.com/gluon/void-oscq/releases), with one file per system. The apps are not signed: your system warns you the first time you open one, and the steps below get past that warning once.

Every download carries the license texts: the GNU GPL, its additional permission and the licenses of the bundled fonts, in a `licenses` folder, inside `OSCQuery Inspector.app/Contents/Resources` on macOS.

### macOS 12 or later, Apple Silicon or Intel

Download `oscq-inspector-VERSION-macos-universal.zip`. Unzip it to get **OSCQuery Inspector.app**, and move it to your Applications folder.

The app is not signed by an identified developer, so macOS stops it the first time:

1. Double click **OSCQuery Inspector**. macOS says it cannot verify the app. Close that dialog: **Done**, **OK** or **Cancel** depending on the macOS version, never **Move to Trash**.
2. Open **System Settings**, then **Privacy & Security**. Scroll down to **Security**: a line says OSCQuery Inspector was blocked. Click **Open Anyway**, then confirm with your password or Touch ID.
3. Click **Open** in the last dialog. From now on the app opens normally.

On macOS 12 to 14 there is a shorter way: Control click the app, choose **Open**, then **Open** again.

The first time the app looks for servers, macOS asks whether OSCQuery Inspector may find devices on your local network. Click **Allow**. If you clicked **Don't Allow**, turn it on later in **System Settings**, **Privacy & Security**, **Local Network**. A server on the same Mac, at `localhost`, works either way.

### Windows 10 or later, 64 bit

Download `oscq-inspector-VERSION-windows-x86_64.zip` and unzip it: a folder holding `oscq-inspector-VERSION-windows-x86_64.exe` and a `licenses` folder. The `.exe` is the whole application: no installer, put it where you like.

The program is not signed, so Windows SmartScreen stops it the first time:

1. Double click the `.exe`. A blue window says **Windows protected your PC**.
2. Click **More info**, then **Run anyway**.

If Windows Defender Firewall asks whether to allow OSCQuery Inspector on networks, allow it on **private networks**: discovery and the values pushed by servers need it.

### Linux, x86_64

Download `oscq-inspector-VERSION-linux-x86_64.tar.gz`, unpack it and run the binary:

```
tar -xzf oscq-inspector-VERSION-linux-x86_64.tar.gz
cd oscq-inspector-VERSION-linux-x86_64
./oscq-inspector
```

To have it in your application menu with its icon, copy the binary to `~/.local/bin`, the `.desktop` file to `~/.local/share/applications`, and `oscq-inspector.png` to `~/.local/share/icons/hicolor/256x256/apps`.

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
- **Presets.** **+ PRESETS** opens the section, folded until then. **Capture** on a slot stores the values of the writable nodes, the pinned ones only when nodes are pinned; **Recall** sends them back, all at once, logged as one block in the Traffic tab. Recalled on another server, only the nodes at the same path with the same type are sent; the status bar counts those not found. No confirmation either way: a capture replaces the slot.
- **Setting the OSC port.** When the server does not advertise its OSC port, the server pane shows "port inferred from the HTTP port" under **OSC destination**, and an OSC port field. Type the port the server receives OSC on, then **Set OSC port** or Enter: the server pane says "port set by the user", sends go there, and the port is kept for that server in the config, used again at every connection and after a restart. **Infer OSC port** goes back to the HTTP port and forgets the setting. A port the server advertises is not editable and always wins, even over a port set earlier: the setting fills a silence of the server, it never contradicts what the server declares.
- **The map.** The Map tab draws every server in the list, whatever its state, and what each one that declares `PEERS` says of its own connections: one arrow per client, from the client to the server, with the number of values it sent; UDP sources that hold no WebSocket in short dashes. A client is drawn as the server whose name it gave; one that gave a name no server has, as the server at its address when exactly one is there, the arrow then saying so; one that gave no name, as an unnamed client at its address and port, never as a server. A name two servers share matches neither, in long dashes. A server connected to itself has a loop on its node. This application is one node, named as itself: every server it is connected to lists it. Each server says how long ago it was read; past three intervals, its arrows turn grey and it says stale. The peers are read when the map opens and every two seconds by default while it is shown, the window waking for that alone; **Refresh** or F5 reads them at once. Scroll to pan, pinch or Ctrl and scroll to zoom, **Fit** to see it whole.
- **Listeners in the tree.** On a server that publishes its peers, a method listened to by other clients carries a badge with their count in the metadata column, their names on hover; the line above the tree says when they were read. The match is exact: a client listening to `/foo` marks `/foo`, not `/foo/bar`, and a container is never marked. This application's own listens are left out, since it listens to every method on screen. On a server that does not publish its peers, the line says so: no badge then means not published, not zero. The listeners are read when the tree shows the server, and with **Refresh**.
- Settings are saved in your system's configuration folder, in `void-oscq/config.json`: window size and place, manual entries, the last selected server, the theme, pinned paths, the OSC ports you set, the preset slots, and `peers_poll_ms`, the interval of the map's readings, 2000 by default, which only the file sets. JSON and traffic exports go to `void-oscq/exports/` beside it. The folder keeps the name the application had before: an installed copy finds its settings again.

## License

Copyright (C) 2026 Julien Bayle, Structure Void. OSCQuery Inspector is free software under the GNU General Public License, version 3 or any later version ([LICENSE](LICENSE)), with an additional permission for the fonts compiled into the application ([LICENSE-EXCEPTION](LICENSE-EXCEPTION)). The fonts keep their own licenses, in [licenses/fonts](licenses/fonts).

## Status

Latest published version: 0.2.3, tagged and released on 2026-10-14, the first whose files are on the Releases page, published under the application's former name. Its macOS download was installed and checked by hand on a Mac against Max 9; earlier versions were checked the same way, and against OSCQuery Helper on a local network. What each check proved is recorded in [docs/acceptance.md](docs/acceptance.md), under "Acceptance record".

Unreleased, on its branch: the map and the listener badges, milestone M7. Proven in CI against the synthetic server; not yet checked by a person against two applications that implement the extension, one a client of the other, which is what [docs/acceptance.md](docs/acceptance.md) section 6 asks.

Backlog:

- Acceptance against a real server that notifies over its WebSocket (VDMX, ossia): a value changed inside it updates in the app without user action. Neither OSCQuery Helper nor Max 9 notifies; the synthetic server is the only proof so far. The known limit and what lifts it are in [docs/acceptance.md](docs/acceptance.md).
- Real captured documents from VDMX and ossia, as test fixtures. OSCQuery Helper and Max 9 are captured.
- OSC sent over the WebSocket, opt in per server, for servers that accept it; OSC goes over UDP only.

## Documents in this repository

This README is the manual. The documents below are the project's working files, left visible on purpose so the reasoning behind the code can be read. They are not user documentation, and some of them describe intent rather than the shipped application:

- [CHANGELOG.md](CHANGELOG.md): what changed in every published version, newest first.
- [PRD.md](PRD.md): the specification the application was built from, written for whoever implements it. Not the manual: where it and the application differ, the application and this README are right.
- [docs/decisions.md](docs/decisions.md): one line per decision taken while building, with its reason.
- [docs/protocol-notes.md](docs/protocol-notes.md): what the OSCQuery proposal says, checked against its sources, and what real servers do.
- [PITFALLS.md](PITFALLS.md): traps met while building, each with the case that produced it.
- [docs/acceptance.md](docs/acceptance.md): the manual checks run before each release, and the record of what they proved.
- [CLAUDE.md](CLAUDE.md): the laws the application is held to, and the review criterion for every change. Not documentation either: it states what must stay true, not how to use the application.
- [tests/fixtures/README.md](tests/fixtures/README.md): the test inputs and where each one comes from.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md).

## Build from source

Rust stable, then:

```
cargo run -p oscq-inspector --release
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
| `crates/oscq-inspector` | the `eframe` application, UI only |

`oscq-core` and `oscq-discovery` never depend on `egui` or `eframe`; CI enforces it. Every crate takes the one workspace version, which a release tag must match.

Protocol answers with their sources are in [docs/protocol-notes.md](docs/protocol-notes.md), including what OSCQuery Helper and Max 9 really do. Decisions are in [docs/decisions.md](docs/decisions.md), traps met in [PITFALLS.md](PITFALLS.md), test fixtures and their provenance in [tests/fixtures/README.md](tests/fixtures/README.md), the release checklist in [docs/acceptance.md](docs/acceptance.md).

### Tools

The application icon is drawn in code, `crates/oscq-inspector/src/icon.rs`: the window icon, the macOS `.icns`, the Windows executable's icon and the Linux `.png` all come from it, and no image file is stored. The release packaging writes the image files with:

```
cargo run -p oscq-inspector --example icons -- OUT_DIR
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

With `--peers serve` it declares the `PEERS` and `CLIENT_NAME` extension and answers `?PEERS` with the clients connected to it; `--peers fail` declares it and answers an error; `--name` sets the name of its `HOST_INFO`:

```
cargo run -p oscq-core --example synthetic_server -- --port 5679 --nodes 200 --peers serve --name Alpha
```

### Performance budgets

The limits the application is held to, against the synthetic server: window visible under 300 ms after start, a 5000 node document parsed under 150 ms, a frame while scrolling it under 8 ms at p95, under 1 percent of one core at rest with 50 values listened to, no repaint at rest, under 2 ms from a slider to its UDP packet, under 150 MB resident with 5000 nodes, a macOS binary under 25 MB. With eight servers publishing 24 peers: a frame of the map under 8 ms at p95, one repaint per poll interval at rest with the map shown, none with it hidden. The measures are in [docs/decisions.md](docs/decisions.md). On a release build. In process, also run in CI:

```
cargo test --release --workspace -- --ignored budget_ --nocapture --test-threads=1
```

The real binary, window included, against the synthetic server; needs a display, Linux or macOS, and the pointer outside the window:

```
cargo test --release -p oscq-inspector --test budgets_binary -- --ignored --nocapture --test-threads=1
```

### Release

The workflow `.github/workflows/release.yml` builds and starts the three artifacts on a tag `v*`, and on any push that changes the packaging. On a tag it then creates a draft GitHub Release holding the three files, with the version's section of [CHANGELOG.md](CHANGELOG.md) as its notes; the draft is published by hand once [docs/acceptance.md](docs/acceptance.md) has passed. The scripts are in `packaging/`. A tag must equal the workspace version, and CHANGELOG.md must have a section for it:

```
git tag v0.2.3
git push origin v0.2.3
```

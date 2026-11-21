# Development

Building OSCQuery Inspector from source, the workspace and its tools, the performance budgets, the release, the status and the backlog, and the project's working files. The user documentation is linked from the [README](README.md).

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

The interface is set in Space Grotesk and data in Space Mono, both under the SIL Open Font License, compiled into the binary so that a window looks the same on the three systems. Their license texts are in `licenses/fonts`, every release carries them, and `ci/check-licenses.sh` checks each one against the family and the copyright its font file declares. The icons are drawn in code, `crates/oscq-inspector/src/glyph.rs`, with no icon font and no icon crate.

The screenshots above are taken from a launch and not by hand: `OSCQ_INSPECTOR_STARTUP` opens a view, so each one is one command. With the window size and the theme set in the config file, and a server to look at:

```
OSCQ_INSPECTOR_STARTUP=connect-all,map ./oscq-inspector
```

`tree`, `json`, `traffic`, `map` and `about` are the views it takes, and `connect-all` connects every known server rather than the selected one alone. The whole set, every view before and after the refresh of 2026-11-17 in both themes and at both widths, is in [docs/images/screenshots](docs/images/screenshots), with how it was taken.

Since 2026-11-19 the window can also write the frame itself, which needs no capture tool, no permission and no desktop of its own, and which the images of [docs/presets-audit.md](docs/presets-audit.md) were taken with. It is behind the `screenshot` feature, so no release build carries it:

```
OSCQ_INSPECTOR_SHOT=out.png OSCQ_INSPECTOR_SHOT_DO=capture:3 oscq-inspector
```

`OSCQ_INSPECTOR_SHOT_DO` takes `select:HOST:PORT`, `expand:/PATH`, `pin:/PATH`, `capture:N` and `recall:N`, one per frame, and the window closes once the file is written.

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
git tag v0.4.0
git push origin v0.4.0
```

## Status and backlog

Latest version: **0.5.1**, dated 2026-11-21, the version the documentation describes: it fixes a setting or a preset capture lost when two saves of the config passed their check at once, a defect 0.5.0 still had, listed in [CHANGELOG.md](CHANGELOG.md). Its tag `v0.5.1` builds the draft Release, published once [docs/acceptance.md](docs/acceptance.md) has passed. **0.5.0**, dated 2026-11-20, brought the new application icon, the preset gestures of Max's `preset` object and the clearing of a slot. Before it, **0.4.2**, tagged on 2026-11-18, carries everything 0.3.0 and 0.4.0 hold, and its changelog entry folds in the two fixes of 0.4.1, which was tagged and built the same day and never published. What the checks before it proved is recorded in [docs/acceptance.md](docs/acceptance.md), under "Acceptance record".

Version 0.3.0 is tagged, at `v0.3.0`, and its draft Release was never published: the acceptance it waits on, section 6, asks for two applications implementing the `PEERS` extension, one a client of the other, and that has not been done by a person. Every later version carries what it holds.

Backlog:

- Acceptance against a real server that notifies over its WebSocket (VDMX, ossia): a value changed inside it updates in the app without user action. Neither OSCQuery Helper nor Max 9 notifies; the synthetic server is the only proof so far. The known limit and what lifts it are in [docs/acceptance.md](docs/acceptance.md).
- Real captured documents from VDMX and ossia, as test fixtures. OSCQuery Helper and Max 9 are captured.
- OSC sent over the WebSocket, opt in per server, for servers that accept it; OSC goes over UDP only.

## Documents in this repository

The user documentation is linked from the README; the manual is [docs/manual.md](docs/manual.md). The documents below are the project's working files, left visible on purpose so the reasoning behind the code can be read. They are not user documentation, and some of them describe intent rather than the shipped application:

- [CHANGELOG.md](CHANGELOG.md): what changed in every published version, newest first.
- [PRD.md](PRD.md): the specification the application was built from, written for whoever implements it. Not the manual: where it and the application differ, the application and its manual are right.
- [docs/decisions.md](docs/decisions.md): one line per decision taken while building, with its reason.
- [docs/protocol-notes.md](docs/protocol-notes.md): what the OSCQuery proposal says, checked against its sources, and what real servers do.
- [PITFALLS.md](PITFALLS.md): traps met while building, each with the case that produced it.
- [docs/presets-audit.md](docs/presets-audit.md): the preset section read from the window on 2026-11-19, what a capture takes and what a recall sends, what the screen does not make readable, and three designs for the capture.
- [docs/acceptance.md](docs/acceptance.md): the manual checks run before each release, and the record of what they proved.
- [CLAUDE.md](CLAUDE.md): the laws the application is held to, and the review criterion for every change. Not documentation either: it states what must stay true, not how to use the application.
- [tests/fixtures/README.md](tests/fixtures/README.md): the test inputs and where each one comes from.

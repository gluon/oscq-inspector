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

The screenshots of the documentation are taken from a launch and not by hand: `OSCQ_INSPECTOR_STARTUP` opens a view, so each one is one command. With the window size and the theme set in the config file, and a server to look at:

```
OSCQ_INSPECTOR_STARTUP=connect-all,map ./oscq-inspector
```

`tree`, `json`, `traffic`, `map` and `about` are the views it takes, and `connect-all` connects every known server rather than the selected one alone. The whole set, every view before and after the refresh of 2026-11-17 in both themes and at both widths, is in [docs/images/screenshots](docs/images/screenshots), with how it was taken.

Since 2026-11-19 the window can also write the frame itself, which needs no capture tool, no permission and no desktop of its own, and which the images of [docs/presets-audit.md](docs/presets-audit.md) were taken with. It is behind the `screenshot` feature, so no release build carries it:

```
OSCQ_INSPECTOR_SHOT=out.png OSCQ_INSPECTOR_SHOT_DO=capture:3 oscq-inspector
```

`OSCQ_INSPECTOR_SHOT_DO` takes `select:HOST:PORT`, `expand:/PATH`, `pin:/PATH`, `capture:N`, `recall:N`, `clear:N` and the views of `OSCQ_INSPECTOR_STARTUP`, one per frame, and the window closes once the file is written. The images of the README were taken that way, on Linux, with the commands in [docs/images/readme](docs/images/readme/README.md).

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

`--doc FILE` publishes the document of FILE instead, and `--stream` then changes its float methods alone, each within its `RANGE`; `crates/oscq-core/examples/demo-space.json` is the address space of a small audiovisual rig, written for the documentation images. With `--echo`, every OSC message the server receives goes back over the WebSocket to the clients listening to its path, as a server confirming a change:

```
cargo run -p oscq-core --example synthetic_server -- --port 5680 --peers serve --echo --doc crates/oscq-core/examples/demo-space.json --name "AV rig" --stream 4
```

`--host-info FILE` serves the JSON of FILE as the whole `HOST_INFO`, byte for byte, in place of the one the server would write. It is how a `HOST_INFO` no server should send is put in front of the application: an attribute present and empty, a port that is not a number, an extension declared and not answered. `--name` then does not reach it, and what it declares, `EXTENSIONS` included, is the file's; the server prints the file it is serving so that there is no doubt which `HOST_INFO` is in play. Beside the example, `host-info-empty-ws-ip.json` is a `WS_IP` sent as an empty string, which names no host and counts as absent, and `one-method.json` a document of a single method:

```
cargo run -p oscq-core --example synthetic_server -- --port 5690 --doc crates/oscq-core/examples/one-method.json --host-info crates/oscq-core/examples/host-info-empty-ws-ip.json
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

The workflow `.github/workflows/release.yml` builds and starts the three artifacts on a tag `v*`, and on any push that changes the packaging. On a tag, the job `macos-sign` then signs the macOS bundle with a Developer ID, notarises it, staples the ticket and checks the three with `codesign`, `spctl` and `stapler`; its seven secrets, named in the workflow, are in the GitHub environment `apple-signing`, which accepts tags `v*` only, so no job of a branch push can read them. A push without a tag zips the macOS bundle as `...-macos-universal-unsigned.zip`. On a tag the workflow then creates a draft GitHub Release holding the three files, with the version's section of [CHANGELOG.md](CHANGELOG.md) as its notes; the draft is published by hand once [docs/acceptance.md](docs/acceptance.md) has passed. The scripts are in `packaging/`. A tag must equal the workspace version, and CHANGELOG.md must have a section for it:

```
git tag v0.4.0
git push origin v0.4.0
```

## Status and backlog

Latest version: **0.7.0**, dated 2026-11-25, the version the documentation describes: a server declaring `WS_IP` as an empty string is followed over its WebSocket again, where the address built from it could not be connected to, and every warning the status bar counts is named when the count is hovered, a `HOST_INFO` warning now standing there without a tree. Its tag `v0.7.0` builds the draft Release, published once [docs/acceptance.md](docs/acceptance.md) has passed, sections 3b and 3c included. **0.6.0**, dated 2026-11-23: the File menu, with projects saved and opened, and the server list, a document, the traffic and the presets exported or imported through the system's dialogs; stale servers removed from the list; 64 manual entries; and the first macOS app signed with a Developer ID and notarised. Its tag `v0.6.0` builds the draft Release, published once [docs/acceptance.md](docs/acceptance.md) has passed. **0.5.1**, dated 2026-11-21, fixes a setting or a preset capture lost when two saves of the config passed their check at once, a defect 0.5.0 still had, listed in [CHANGELOG.md](CHANGELOG.md). **0.5.0**, dated 2026-11-20, brought the new application icon, the preset gestures of Max's `preset` object and the clearing of a slot. Before it, **0.4.2**, tagged on 2026-11-18, carries everything 0.3.0 and 0.4.0 hold, and its changelog entry folds in the two fixes of 0.4.1, which was tagged and built the same day and never published. What the checks before it proved is recorded in [docs/acceptance.md](docs/acceptance.md), under "Acceptance record".

Version 0.3.0 is tagged, at `v0.3.0`, and its draft Release was never published: the acceptance it waits on, section 6, asks for two applications implementing the `PEERS` extension, one a client of the other, and that has not been done by a person. Every later version carries what it holds.

Backlog:

- The map exported as SVG, from the File menu. Asked for on 2026-11-23, left out of the File menu's first version.

- Acceptance against a real server that notifies over its WebSocket (VDMX, ossia): a value changed inside it updates in the app without user action. Neither OSCQuery Helper nor Max 9 notifies; the synthetic server is the only proof so far. The known limit and what lifts it are in [docs/acceptance.md](docs/acceptance.md).
- Real captured documents from VDMX and ossia, as test fixtures. OSCQuery Helper and Max 9 are captured.
- OSC sent over the WebSocket, opt in per server, for servers that accept it; OSC goes over UDP only.
- The Developer ID signature, the notarisation and the stapled ticket of the downloaded bundle checked by `ci/accept-artifact.sh`, with a refusal on a bundle altered after signing. Not written: at `v0.7.0` the script calls neither `codesign`, nor `spctl`, nor `stapler`, and no commit of it ever has, its last change being `4ac2bb6` of 2026-11-20. What it does check is what [docs/acceptance.md](docs/acceptance.md) section 0 lists, and that passed on the draft of `v0.6.0`, run 45234573661, and on the draft of `v0.7.0` on the Mac; run it again against the draft of each signed tag after it. Until the three are in the script they are a person's, and were run by hand on both tags: `spctl` reporting `source=Notarized Developer ID`, `stapler validate` worked, `codesign --verify --deep --strict` valid on disk.
- The Traffic tab once listed the values a server sent back above the row of the preset recall that sent them, at the same millisecond: one launch out of three, recorded in [docs/images/readme](docs/images/readme/README.md). Not reproduced on demand; the cause is not established.
- The `Lost` state of the WebSocket told apart from `Absent` in a narrow window. `values_text` in `crates/oscq-inspector/src/app.rs` gives `Absent` the string `values on refresh only` and `Lost` that same string followed by `: WebSocket lost (reason), retry in N s`, so a cell too narrow for the second half shows the first and the two states read alike, one meaning nothing is coming and the other that a retry is scheduled. Only the colour separates them, `style::SIGNAL` for `Lost`. Met on 2026-11-25 while running acceptance 3b step 4, where a retrying socket was read as a dead one and the section was nearly recorded as failed. Candidates: the retry first in the string, a minimum width for that cell, or 3b saying that orange means read the whole line.
- A WebSocket address that was not advertised gets one attempt and no retry: in `crates/oscq-core/src/service.rs`, `Err(e) if !advertised && !opened_before` returns silently on the first failure, line 1021 at `v0.7.0`. A server whose `WS_IP` is an empty string falls into that bucket by design, a fallback never passing for a declaration of the server's, so if its single attempt loses a race with a server still starting up, the window stays on values on refresh only with nothing scheduled and no way back but a reconnection by hand. Not observed: found while accounting for 3b step 4, which turned out to be the retrying state instead. A test would refuse the first connection and accept the second.
- The synthetic server prints `1 methods holding a value`, `crates/oscq-core/examples/synthetic_server.rs:140`: the plural of one that 0.7.0 fixed in the status bar, still in the repository's own tool. Not read by any acceptance section.

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

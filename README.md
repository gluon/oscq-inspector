# Void OSCQ

A native OSCQuery client for macOS, Windows and Linux: discover servers on the LAN, browse their OSC address space, see live values, and send OSC. The product specification is [PRD.md](PRD.md).

## Workspace

| Crate | Role |
| --- | --- |
| `crates/oscq-core` | protocol: address space model, document parser, HTTP client, `HOST_INFO`, WebSocket client, OSC sending |
| `crates/oscq-discovery` | mDNS browsing and manual entry, depends on `oscq-core` |
| `crates/void-oscq` | the `eframe` application, UI only |

`oscq-core` and `oscq-discovery` never depend on `egui` or `eframe`; CI enforces it.

Protocol answers with their sources are in [docs/protocol-notes.md](docs/protocol-notes.md), decisions in [docs/decisions.md](docs/decisions.md), test fixtures and their provenance in [tests/fixtures/README.md](tests/fixtures/README.md).

## Build and test

```
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

Linux build dependencies, as installed in CI:

```
sudo apt-get install -y libxkbcommon-dev libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev
```

Linux run time, X11: `libxkbcommon-x11-0` and a Vulkan or OpenGL driver.

## Run the application

```
cargo run -p void-oscq --release
```

Servers found by mDNS are listed on the left; `+ manual` connects to a `host:port`. Max 9 serves OSCQuery on `localhost:30339` without advertising it: add it by manual entry. Settings are saved in the OS config directory, `void-oscq/config.json`; JSON and traffic exports go to `void-oscq/exports/` beside it.

## Discover servers

`oscq_discover` lists OSCQuery and OSC servers live, with each address and the interface it was seen on, and a summary two seconds after start:

```
cargo run -p oscq-discovery --example oscq_discover
```

## Synthetic server

The test suite's OSCQuery server, 5000 methods by default, HTTP and WebSocket on one port. With `--stream HZ` it changes the value of every method the app listens to, HZ times a second:

```
cargo run -p oscq-core --example synthetic_server -- --port 5678 --stream 2
```

## Performance budgets

PRD 10 budgets, on a release build. In process, also run in CI:

```
cargo test --release --workspace -- --ignored budget_ --nocapture --test-threads=1
```

The real binary, window included, against the synthetic server; needs a display, Linux or macOS:

```
cargo test --release -p void-oscq --test budgets_binary -- --ignored --nocapture --test-threads=1
```

Measured numbers are in [docs/decisions.md](docs/decisions.md).

## Probe a server

`oscq_probe` prints the `HOST_INFO` summary and the full tree of a server, and can send one value:

```
cargo run -p oscq-core --example oscq_probe -- http://HOST:PORT
cargo run -p oscq-core --example oscq_probe -- http://HOST:PORT --send /path ARG
```

Arguments are parsed against the node's `TYPE`, one per top level type tag.

## Status

- M0, protocol groundwork: done, merged.
- M1, core client: done, merged. `cargo test` covers the parser, `HOST_INFO`, attribute queries and UDP sending against a synthetic server and real OSCQuery Helper captures. Proven by the human on 2026-10-09: `oscq_probe` printed the Helper's 22 node tree and a value sent to `/test/my_float` reached the Helper's target.
- M2, discovery: done, merged. Proven on the human's LAN on 2026-10-09: one entry in 0.18 s, lo0 and en0 merged, LAN IPv4 promoted.
- M3, application: merged. `cargo test` covers the network service, the widget table, the tree model, the JSON slicing, the traffic log, rate limiting, the config, and the UI headless (idle frames, keyboard, bounded event drain, a sent value kept on a write only node). Tested by the human on the Mac against OSCQuery Helper on 2026-10-09: sends correct byte for byte on `/test/my_longlong` (`,h`); the widget jumped back after release on write only nodes, fixed since, not yet proven on the Mac.
- M4, WebSocket and performance: on branch `void/m4-websocket`. `cargo test` covers the Max 9 captures, `HOST_INFO` answered with the tree, the WebSocket client against the synthetic server (listens, value frames, structure commands, drop and reconnection, advertised and refusing, hostile frames, silent fallback), the listen discipline of the window, and a value pushed by the server shown without user action. Budgets measured, see docs/decisions.md. Tested by the human against Max 9 by manual entry: on 2026-10-09 with the M3 build, "port inferred from the HTTP port" shown, tree loaded with its values; on 2026-10-11 with this branch, the WebSocket attempt on the HTTP port accepted, and Max found to send no notification, see docs/protocol-notes.md section 12. A socket is now shown live only once the server has sent something; that display is not yet seen on the Mac. Not proven: a value changed in a real server appearing live, which needs a server that notifies.

Backlog:

- Real captured documents from VDMX and ossia, PRD 12. OSCQuery Helper and Max 9 are captured.
- M3 acceptance on the human's Mac: browse OSCQuery Helper, change a value, JSON and traffic tabs correct.
- M4 acceptance: a value changed inside a real server that notifies over its WebSocket (VDMX, ossia; not Max 9, which sends nothing) updates in the app without user action.
- PRD 10 on the human's Mac after `calm_style`: idle repaints and idle CPU to measure again, Max and the Helper closed, the pointer outside the window. Cold start closed, 132.8 ms at worst.
- OSC sent over the WebSocket, opt in per server (PRD 7.5).
- M5 release, as in PRD 14.

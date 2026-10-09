# Void OSCQ

A native OSCQuery client for macOS, Windows and Linux: discover servers on the LAN, browse their OSC address space, see live values, and send OSC. The product specification is [PRD.md](PRD.md).

## Workspace

| Crate | Role |
| --- | --- |
| `crates/oscq-core` | protocol: address space model, document parser, HTTP client, `HOST_INFO`, OSC sending |
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

## Discover servers

`oscq_discover` lists OSCQuery and OSC servers live, with each address and the interface it was seen on, and a summary two seconds after start:

```
cargo run -p oscq-discovery --example oscq_discover
```

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
- M2, discovery: merged. `cargo test` covers deduplication, stale marking, address order, manual history, and real mDNS on the build machine, including a server killed without a goodbye. Not yet proven on the human's LAN.

Backlog:

- Real captured documents from VDMX and ossia, PRD 12. OSCQuery Helper is captured.
- M2 acceptance on the human's LAN: `oscq_discover` lists OSCQuery Helper as exactly one entry within two seconds.
- M3 application, M4 WebSocket and performance, M5 release, as in PRD 14.

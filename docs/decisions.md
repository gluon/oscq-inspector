# Decisions

One line per decision. Newest entries at the bottom. A dependency not listed in PRD.md section 5, a relaxed performance budget, or a deviation from PRD.md gets an entry here with its reason.

| Date | Decision | Rationale |
| --- | --- | --- |
| 2026-10-09 | Rust, stable toolchain, edition 2024 | One binary, no runtime to install, predictable latency, same code on three platforms (PRD 5, LOCKED) |
| 2026-10-09 | GUI: `eframe` / `egui` | Immediate mode, no webview, trivial cross compilation, built in virtualised list primitives (PRD 5, LOCKED) |
| 2026-10-09 | Async runtime: `tokio` multi thread, network layer only | HTTP, WebSocket and timers in one place, UI thread never blocks (PRD 5 and 6.2, LOCKED) |
| 2026-10-09 | HTTP client: `reqwest` with rustls | No OpenSSL on any platform; in reqwest 0.13 the default TLS backend is rustls, `cargo tree -i openssl-sys` finds nothing (PRD 5, LOCKED) |
| 2026-10-09 | WebSocket: `tokio-tungstenite` with rustls (`rustls-tls-webpki-roots`) | Text frames for commands, binary frames for OSC packets (PRD 5, LOCKED) |
| 2026-10-09 | OSC encode and decode: `rosc` | Mature, no unsafe, bundles and common type tags (PRD 5, LOCKED) |
| 2026-10-09 | mDNS and DNS-SD: `mdns-sd` | Pure Rust, identical on three platforms, no Avahi or Bonjour SDK (PRD 5, LOCKED) |
| 2026-10-09 | JSON: `serde` and `serde_json` | Documents deserialised into typed structs, never walked untyped in UI code (PRD 5, LOCKED) |
| 2026-10-09 | Logging: `tracing`, `tracing-subscriber` with env filter | Needed to debug other people's servers (PRD 5, LOCKED) |
| 2026-10-09 | Errors: `thiserror` in library crates, `anyhow` in the binary | Library errors are matched on, application errors are reported (PRD 5, LOCKED) |
| 2026-10-09 | Fuzzy matching: `nucleo-matcher` | Path search over thousands of nodes stays interactive (PRD 5, LOCKED) |
| 2026-10-09 | Workspace of three crates: `oscq-core`, `oscq-discovery`, `void-oscq`; only `void-oscq` depends on `egui` / `eframe` | Protocol testable without a GUI, reusable by a headless tool (PRD 6.1, LOCKED) |
| 2026-10-09 | Versions added with `cargo add`, `Cargo.lock` committed, CI runs with `--locked` | Exact resolved versions live in the lock file, not in the PRD (PRD 5) |
| 2026-10-09 | `clippy::unwrap_used` denied workspace wide, allowed in tests via `clippy.toml` | Enforces "No `unwrap` outside tests" (PRD 11) as a gate rather than a review rule |
| 2026-10-09 | All crates `publish = false` | Crate name to be verified again before any publication (PRD header) |

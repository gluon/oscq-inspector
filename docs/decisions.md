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
| 2026-10-09 | PRD amended with the rulings on the M0 discrepancies; `crossbeam-channel` and `url` added to the PRD 5 table | Human ruling: the proposal wins, VVOSCQueryProtocol wins where the proposal contradicts itself |
| 2026-10-09 | `oscq-discovery` depends on `oscq-core`, never the reverse | Human ruling, shared types live in `oscq-core` (PRD 6.1) |
| 2026-10-09 | `serde_json` feature `unbounded_depth`, with a non recursive depth scan rejecting JSON nested deeper than 160 before parsing | The default limit of 128 is below the 64 node cap (each node nests twice); the scan bounds stack use instead. Parsing at the limit is tested on a 512 KiB stack |
| 2026-10-09 | Nodes deeper than 64 keep an empty `CONTENTS` and get a `DepthLimit` diagnostic | PRD 11 "cap depth at 64 and report": the rest of the tree stays browsable |
| 2026-10-09 | Problems that do not prevent parsing are diagnostics, not errors; the offending raw value is kept in `extra` | PRD 7.1 "render what is there"; the raw body is also kept whole for the JSON tab |
| 2026-10-09 | Missing `FULL_PATH` is derived from the parent path and the child key, with a diagnostic; not reported on `OVERLOADS` entries | The proposal's own `OVERLOADS` example and OSCQuery Helper's sample document carry no `FULL_PATH` |
| 2026-10-09 | A node with both `CONTENTS` and `TYPE` is a container and a method | Not ruled by the human. VVOSCQueryProtocol nodes carry both, and Vidvox oscqueryhtml treats such a node as a control (`test/extract.js`, "has contents and type", commit `ca25041`) |
| 2026-10-09 | OSC addresses validated strictly against OSC 1.0: printable ASCII, none of `space # * , / ? [ ] { }`, no empty part | PRD 11 case "characters legal in JSON but not in OSC"; such nodes are shown and never sent to |
| 2026-10-09 | `TYPE` accepts one leading `,`; array groups nested deeper than 16 are rejected | The wire form of a type tag string starts with `,`; the cap bounds every recursion over a signature |
| 2026-10-09 | Coercion: a non integral number sent to `i` or `h` is rounded to nearest, ties away from zero, then range checked | PRD 7.5 "never send a float where the node declared an int"; the rounding rule is this project's choice |
| 2026-10-09 | `S` (symbol) is refused on send | `rosc` 0.11 has no symbol type; sending it as `s` would change the type tag |
| 2026-10-09 | `OSC_TRANSPORT` compared case insensitively; any other value is a diagnostic and UDP is assumed | The proposal defines `"TCP"` and `"UDP"` only; VVOSCQueryProtocol falls back to UDP |
| 2026-10-09 | `OSC_PORT` present but not a valid port makes the server not controllable, it is not inferred | The ruling infers the port only when the key is absent |
| 2026-10-09 | Ports given as numeric strings are accepted | VVOSCQueryProtocol client does the same (`VVOSCQueryRemoteServer.mm`) |
| 2026-10-09 | An attribute query answered 200 with keys other than the queried one is treated like an empty object | A server that ignores the query must not be reported as broken |
| 2026-10-09 | HTTP client: no proxy, connect timeout 3 s, request timeout 10 s, body cap 64 MB | OSCQuery servers are on the LAN, a system proxy would misroute; the timeouts are this project's choice |
| 2026-10-09 | OSC destination resolution prefers IPv4, falls back to the first address | Matches the discovery preference in PRD 8.2 |
| 2026-10-09 | `CONTENTS` children are kept sorted by name | `serde_json` without `preserve_order` does not keep server order; enabling it would add `indexmap` |
| 2026-10-09 | Dev dependencies of `oscq-core`: `anyhow` for the example, `tokio` features `macros` and `io-util` for the synthetic server | `anyhow` is in PRD 5 for binary boundaries; no new crate |
| 2026-10-09 | `flume` replaces `crossbeam-channel` as the network to UI channel and the `Discovery` receiver, in PRD 5, 6.2 and 8.1 | Human ruling on the M1 conflict: `mdns-sd` returns a `flume::Receiver`, which covers the same need and avoids a bridge thread in M2. Supersedes the `crossbeam-channel` part of the entry above |
| 2026-10-09 | `flume` in both directions: UI to network `flume::Sender<Command>` received with `recv_async`, network to UI `flume::Sender<Event>`; `tokio::sync::mpsc` dropped from PRD 6.2 | Human ruling: one channel crate for the whole project |
| 2026-10-09 | `flume` declared once in `[workspace.dependencies]`, version 0.12 without default features, feature `async`; `oscq-core` uses it with `workspace = true` | Same 0.12 series as `mdns-sd`, so `Discovery` returns the very receiver type `mdns-sd` produces; `async` provides `recv_async`. Other crates add `flume.workspace = true` when they use a channel |
| 2026-10-09 | `ServerId` (process unique counter) and `DiscoveredServer` with its address types live in `oscq-core` | Shared by discovery and the future `Event` vocabulary (PRD 6.3); `oscq-discovery` depends on `oscq-core` |
| 2026-10-09 | `flume` workspace feature `select` added | The discovery worker waits on both browse receivers, the stop signal and the liveness timer with one `flume::Selector` in one thread |
| 2026-10-09 | Deduplication key: service kind, host name lower cased without the trailing dot, port. The instance name is display only | PRD 8.2 "by host and port, not by instance name"; the kind is part of the key because `_osc._udp` and `_oscjson._tcp` on one port are different services |
| 2026-10-09 | Addresses are never discarded. Order: IPv4, IPv6, IPv6 link local, then loopback and the Docker default bridge 172.17.0.0/16 as fallbacks | PRD 8.2 reads "skip loopback and Docker ... keep the others as fallbacks"; read as never preferred but kept, so a server on the same machine with no LAN still works. The rest of 172.16.0.0/12 is ordinary private space and is not demoted. Interpretation to confirm |
| 2026-10-09 | A server's address list is the union of everything seen for it; interfaces are ordered by OS index | `mdns-sd` resolves one interface at a time (observed: loopback first, LAN second); a union keeps the earlier one as a fallback |
| 2026-10-09 | Every `ServiceRemoved` marks the server stale; stale servers are kept, never deleted automatically, and revive with the same `ServerId` | `mdns-sd` 0.21 emits the same event for a goodbye and for cache expiry, so they cannot be told apart; PRD 8.2 "mark stale, let the user retry" |
| 2026-10-09 | Liveness check: `mdns-sd` `verify` on every advertised instance every 15 s, answer timeout 10 s (`VERIFY_TIMEOUT_DEFAULT`, RFC 6762 10.4); on by default on Windows only, configurable elsewhere | PRD 8.2 Windows fallback. Proven by a test that kills a publisher process: stale with the check, still listed without it over the same window |
| 2026-10-09 | Loopback interfaces are browsed (the `mdns-sd` default) | A server on the same machine is found; the real OSCQuery Helper capture showed it on interface 1, the loopback, and on interface 14 |
| 2026-10-09 | `DiscoveryEvent` is `Found`, `Changed`, `Stale`, each with a full server snapshot | PRD 6.3 names `ServerFound` and `ServerLost`; discovery never loses a server, it marks it stale. Mapping to the `Event` vocabulary is left to M3 |
| 2026-10-09 | Manual entries: `host:port` or `[IPv6]:port`, always an OSCQuery server; history of 10, most recent first, case insensitive duplicates removed; persistence left to the application config | PRD 8.2; the config file is PRD 9.7, built in M3 |
| 2026-10-09 | An explicit `ACCESS` always wins; no heuristic overrides it. A `VALUE` present on a write only node is a server inconsistency, not a signal of readability | Human ruling, after the real OSCQuery Helper capture: the Helper declares `ACCESS` 2 on every node by design, `OSCQueryHelper/OSCQueryHelperAppDelegate.m` lines 358 to 360 in VVOSCQueryProtocol commit `cebb6f8`, "access is always write-only in this application (we can't read the remote app's OSC address space)", yet serves `VALUE` on its `T` and `F` nodes |
| 2026-10-09 | PRD 6.3: `Event::ServerLost(ServerId)` replaced by `ServerChanged` and `ServerStale`, each carrying a `DiscoveredServer` whose `stale` flag holds the state | Human ruling after M2: discovery marks stale and never removes, the `Event` vocabulary matches `DiscoveryEvent` (`Found`, `Changed`, `Stale`). Supersedes the mapping left open in the `DiscoveryEvent` entry above |
| 2026-10-09 | The three M2 interpretations are confirmed: loopback and Docker kept as fallbacks, service kind in the deduplication key, Docker demotion limited to 172.17.0.0/16 | Human ruling after M2 acceptance on the LAN: one entry in 0.18 s, lo0 and en0 merged, LAN IPv4 promoted |

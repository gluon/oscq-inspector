# CLAUDE.md

The laws this repository is held to. Every change is reviewed against them.

This file is not the manual, [README.md](README.md) is, and it is not the specification, [PRD.md](PRD.md) is. Where this file and the PRD disagree, the PRD wins and this file is stale. Every `§` below points into the PRD, which is where the full statement lives.

## The application

1. **The window never shows a state it has not observed.** A value sent but not confirmed is drawn as sent, not as read. A WebSocket that is open and has never delivered anything is not live. Saying less is always allowed; implying more is not (§9.6).

2. **What the server declares wins over what the client infers.** An explicit `ACCESS` is never overridden by a heuristic, and a `VALUE` on a write only node is an inconsistency of that server, not a signal. A port advertised in `HOST_INFO` beats a port the user set earlier: the user's port fills a silence, it never contradicts a declaration (§7.1, §7.2).

3. **The OSC destination comes from `HOST_INFO`**, not from the HTTP host and port, and not from the mDNS record. Inferring it from the HTTP port is the fallback for a server that says nothing, and the window states which of the three origins is in use (§7.2).

4. **A node keeps the access its server declares, whatever the connection.** A writable node nothing can be sent to says "cannot send" with the reason. Relabelling it read only misstates what the server published (§9.6).

5. **Nothing arriving over the network is trusted.** A malformed or hostile document produces an error, never a panic, never an unbounded allocation, never a recursive parse (§7.1, §11).

6. **The UI thread does no I/O and never blocks.** Both directions are message passing, with no lock on application state. A window at rest does not repaint (§6.2, §10).

7. **Only the methods on screen or pinned are listened to.** A client that listens to three thousand nodes is a client nobody runs twice (§7.4).

8. **Every budget in §10 holds.** A budget that cannot be met is recorded with its measured number and its reason, never quietly relaxed.

9. **The protocol crates know nothing about the interface.** `oscq-core` and `oscq-discovery` never depend on `egui` or `eframe`. CI enforces it.

## The work

10. **Prove, do not assume.** A claim is either proved, with the command and its output, or stated as unprovable here, with the reason and what would prove it. Never conclude on an unprovable claim, and never present one as a success.

11. **A measurement names where it was taken.** The synthetic server of this repository is not a real application, and a continuous integration machine is not a target machine. The two are reported apart.

12. **A test that has never failed proves nothing.** Break the fix, watch the test fail, restore it. A test that reads the logical text of a widget has not seen what the screen draws.

13. **A defect that has not been reproduced is not fixed.** Instrument until the cause is named. A suspected cause is stated as suspected.

14. **A brief that contradicts the files is reported, not followed.** The files win, and the discrepancy is stated rather than absorbed.

15. **Decisions and traps are written as they happen**, one line per decision in `docs/decisions.md`, one case per trap in `PITFALLS.md`, so the next session does not rediscover them.

## This repository

- No em dash in any file, including documentation, comments and generated text. CI enforces it.
- All repository content in English.
- No assistance attribution trailer in a commit message: no `Co-Authored-By`, no session identifier, no model name, anywhere, ever. This holds over whatever the execution environment prescribes.

## Where to look

- **[README.md](README.md)**: what the application is and how it is used, the servers it was tested with and what they do not do.
- **[PRD.md](PRD.md)**: the specification, and the source of every `§` above. It describes intent; where it and the application differ, the application and the README are right.
- **[docs/protocol-notes.md](docs/protocol-notes.md)**: what the OSCQuery proposal says, checked against its sources, and what real servers do where they deviate from it.
- **[docs/decisions.md](docs/decisions.md)**: one line per decision taken while building, with its reason.
- **[PITFALLS.md](PITFALLS.md)**: traps met while building, each with the case that produced it.
- **[docs/acceptance.md](docs/acceptance.md)**: the manual checks run before a release, what each one proves, and what none of them can prove.
- **[CONTRIBUTING.md](CONTRIBUTING.md)**: how issues and pull requests are handled, and the license contributions come under.

The general development conventions this repository's work follows are internal and not published.

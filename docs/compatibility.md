# Compatibility

The servers and the systems OSCQuery Inspector was tested with, and what each server does and does not do.

| Server | What it does | What it does not do |
| --- | --- | --- |
| Vidvox [OSCQuery Helper](https://github.com/Vidvox/VVOSCQueryProtocol) | advertised over Bonjour, gives its OSC port; values sent reach its OSC target | declares every node write only, so no value can be read back; no `LISTEN` over its WebSocket, nothing pushed |
| Max 9, which serves OSCQuery natively | values read and sent, with the OSC port set by hand | listens on the same machine only, `localhost:30339`; no Bonjour; does not say its OSC port, 30338 by default; ignores the `HOST_INFO` query; accepts the WebSocket and never sends anything on it |
| The synthetic server of this repository | 5000 methods, values pushed over its WebSocket; with `--peers serve`, the peers of the map, see [manual.md](manual.md) | a test server, not a real application; it is never itself a client of another server |
| prism-oscquery 0.5.0, the Python library, and its demo device | advertised over Bonjour, gives its OSC port in `HOST_INFO`; values read and sent; values pushed over its WebSocket, which the Traffic tab logs as they arrive; publishes its peers, so it is drawn on the map with its edges | its demo device publishes five methods, which says nothing about a large address space |

ossia-prism 2.1.0+prism.14 pushes its values as well. That one is reported by the author and is not exercised in this repository's own runs, so it is named here and not in the table above.

Not tested: VDMX, ossia upstream and any other server. Of the servers above, the synthetic one and the prism-oscquery demo publish their peers: the map and the listener badges are proven against those two. Reports of how the app behaves with the others are welcome, see [CONTRIBUTING.md](../CONTRIBUTING.md).

## Systems

- **macOS**: tested by hand.
- **Linux**: built, tested and started in CI under Xvfb, a virtual display.
- **Windows**: built and tested without its window in CI; since 0.2.3, every release build unzips the packed executable and starts it, and fails unless it draws its first frame. Never tried by hand.

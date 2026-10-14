# Contributing

Void OSCQ is developed by Julien Bayle for his own use, building and debugging OSC setups, and published in case it serves others. There is no team behind it and no roadmap beyond the backlog in the [README](README.md#status).

## Issues

Issues are read. The most useful ones say:

- the version of Void OSCQ, from its About box, and the system it runs on;
- the server: its name and version, and whether it is advertised over Bonjour or added by hand;
- what you did, what you expected, what you saw; the status bar's message, and the Traffic tab exported as text when messages are involved.

A report of how the app behaves with a server it was not tested with, VDMX, ossia or any other, is welcome even when nothing fails: the [tested servers](README.md#tested-with) are three. A capture of that server's documents is better still, `curl -s http://HOST:PORT/` and `curl -s "http://HOST:PORT/?HOST_INFO"`; check that it holds nothing you would rather keep private before attaching it.

## Pull requests

Pull requests are read, but there is no promise that one will be merged: what goes in is decided by what the tool is for. Open an issue first for anything beyond a small fix, so that no work is wasted on a change that would be declined.

A pull request passes the checks CI runs on the three systems:

```
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

Repository text and code are in English. The [documents in this repository](README.md#documents-in-this-repository) explain why the code is as it is.

## License

Void OSCQ is under the GNU General Public License, version 3 or any later version ([LICENSE](LICENSE)), with an additional permission under its section 7 for the fonts compiled into the application ([LICENSE-EXCEPTION](LICENSE-EXCEPTION)). A contribution is accepted under the same terms, the additional permission included.

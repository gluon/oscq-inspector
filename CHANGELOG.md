# Changelog

Every published version of OSCQuery Inspector, formerly void-oscq and published under that name up to 0.2.3, newest first. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html). A version's date is the date of its tag.

## [Unreleased]

### Added

- **Map**, a fourth tab: every server, and what each one that publishes its peers says of its connections, one arrow per client, UDP sources apart, this application named as itself. Read when the tab opens, then every two seconds while it is shown, never behind another tab; each server says how old its reading is, and past three intervals it is marked stale. A server that does not publish its peers is drawn and says so, with no arrow.
- **Listeners in the tree**: on a server that publishes its peers, a badge on each method other clients listen to, with their count, their names on hover; the line above the tree says when they were read, or that the server does not publish them.
- **`PEERS` and `CLIENT_NAME`**: the application names itself to the servers that declare the extension, on every connection, and reads their peers. Nothing of it is sent to a server that does not declare it.
- **`peers_poll_ms`** in the settings file, the interval of the map's readings, 2000 by default.
- **A map you handle**: drag a node and it stays where you put it, across launches, until **Release**; click a node to light it with its arrows and neighbours; click a server's node to select it, double click to open its tree.

### Fixed

- **Failures in the log**: every failure the window shows is also written to the log, whole, naming its server, and a plain launch without `RUST_LOG` now logs at `warn`; before, it logged errors only, and a failed reading of a server's peers was logged nowhere.
- **Failures of the map in the status bar**: a server whose peers cannot be read says so in the status bar, the selected server's on every tab and every other server's on the map, the reason whole on hover. Its hover no longer shows the message twice.

### Changed

- **The map's places**: nodes are placed by forces, those connected together drawn together, spread over the map's area, and still once placed. A server carries the name its `HOST_INFO` declares, even when entered by hand, its address below.
- **A readable map**: every server's name is always drawn at a readable size. When the map does not hold with every detail readable, it is drawn compact, a server as its name, a peer as a dot with its name, the rest on hover; when room is short an edge's label goes first, then a client's name, never a server's. **Fit** shows the whole map when it can be read, the rest is panned to otherwise. A text too small to read is not drawn.
- **Name**: the application is OSCQuery Inspector. Its crate, its binary, its downloads and its `.desktop` file are `oscq-inspector`; on macOS the bundle is **OSCQuery Inspector.app**. The macOS bundle identifier and the settings folder keep the former name, so that an installed copy stays the same application to its system and finds its settings again.

## [0.2.3] - 2026-10-14

The first version whose files are on the Releases page.

### Added

- **Releases**: a tag creates a draft GitHub Release holding the three files, with the version's section of this changelog as its notes; it is published once the release checks have passed.
- **CHANGELOG.md**, this file, in place of the version history the README carried.
- **CONTRIBUTING.md**: how issues and pull requests are handled, and the license contributions come under.

### Changed

- **Writable**: the toolbar check box reads "Writable", no longer "Writable only". The toolbar holds on one line from a window of 1060 points, down from 1090.
- **Windows download**: a zip holding the `.exe` and its `licenses` folder, in place of the bare `.exe`.
- **Windows**: the release build unzips the packed executable and starts it, and fails unless it draws its first frame, as it already did on macOS and Linux. No Windows binary up to 0.2.2 had been started.
- **Documentation**: the README addresses a user first, with the servers the app was tested with and what they do not do, how each system is checked, the downloads and the first launch on each system; the project's working documents are named as such.

## [0.2.2] - 2026-10-13

### Added

- **Writable only**, a check box in the toolbar: the tree and the filter leave out the nodes that cannot be written, and the containers left with none; the status bar counts them; the setting is kept.

### Changed

- **Filter**: a hit's container path shown only when another hit carries the same name in another container; otherwise the name alone.
- **Presets** folded until opened, the pinned panel taking the room; kept open or folded.
- **Light theme** toned: buttons and fields white and edged, rails a firm grey, text darker; controls stand out from the panel at least as much as in the dark theme.

## [0.2.1] - 2026-10-13

### Changed

- **Filter**: each hit named first, then the path of its container, cut from the left when the column is short: hits of one name in different containers stay told apart.
- **Toolbar**: the tabs and Refresh first, then the filter and root fields sharing the rest of the line; on a window of 1000 points or more it holds on one line, and the side panes keep their width.
- **Presets**: each slot names its two actions, **Recall** and **Capture**; a slot holding values is tinted, its number plain, Recall on; an empty one has a faint number and Recall off.

### Added

- **Unpinning from the pinned panel**: a button at the end of each row, besides the right click.

## [0.2.0] - 2026-10-13

### Added

- **Pinned nodes**, kept per server, visible whatever the tree shows.
- **Sixteen preset slots**, captured and recalled in one click, kept in the config; a recall on another server sends what matches and counts the rest.
- **Traffic**: filter by direction; a preset recall logged as one block; right click a row to copy its full path.
- **About box**: name, version, Julien Bayle, Structure Void, license, repository link, from the About button and, on macOS, from the application menu.

### Changed

- **Interface rework**: a 4 px grid and one type scale; a tree in three columns, name, widget, type tags and flags, with indentation guides; the widget holds the value; monospace for data; a one line status bar in fixed zones, details on hover.
- **Narrow windows**: down to the 640 px minimum, no control leaves the screen. The toolbar wraps, Refresh included; the side panes narrow in turn and their handles stop at their limits; a narrow tree gives up the name, then the metadata down to the access flags, never the widget.
- **Server list**: discovered and manual servers apart; a manual entry can be removed, with the OSC port set for it; the OSC destination and the OSC port field, still correctable when the server does not advertise its port, are in the server pane; with no server found, it says since when it has been searching.
- **License**: GPL-3.0-or-later, with an additional permission for the fonts compiled into the application. Every artifact carries the license texts; the Windows artifact holds the `.exe` and a `licenses` folder.

## [0.1.1] - 2026-10-12

### Added

- **Application icon** on the three systems: the macOS `.icns` in the bundle, the Windows executable's icon, the Linux `.png` named by the `.desktop` file, and the window and the Dock while the app runs.
- **OSC port you can set** per server when the server does not advertise one; the status bar tells advertised, inferred from the HTTP port, or set by the user. Needed for Max 9, which receives OSC on 30338 and serves OSCQuery on 30339.

### Fixed

- **Version in the macOS bundle**: both version keys of `Info.plist` carry the release version, checked against the tag by the release workflow.
- **Errors say why**: a failed connection shows the system's reason, such as "Connection refused", not only "error sending request".
- **Errors that no longer hold are cleared**: an error disappears once the same operation succeeds, and a failed connection is said once.
- **Access is the server's**: when the connection fails, nodes keep the access the server declared; writable nodes say "cannot send", with the reason, never "read only". The same for a server declaring OSC over TCP.
- **Release workflow**: `actions/upload-artifact` v7, no Node 20 deprecation warning.

## [0.1.0] - 2026-10-12

### Added

- First release: discovery over Bonjour and manual entries, the address space as a tree, values read and sent with widgets fitting their types, values live over the WebSocket where the server pushes them, the JSON and Traffic tabs. Unsigned artifacts for macOS (universal), Windows and Linux.

[Unreleased]: https://github.com/gluon/void-oscq/compare/v0.2.3...HEAD
[0.2.3]: https://github.com/gluon/void-oscq/compare/v0.2.2...v0.2.3
[0.2.2]: https://github.com/gluon/void-oscq/compare/v0.2.1...v0.2.2
[0.2.1]: https://github.com/gluon/void-oscq/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/gluon/void-oscq/compare/v0.1.1...v0.2.0
[0.1.1]: https://github.com/gluon/void-oscq/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/gluon/void-oscq/releases/tag/v0.1.0

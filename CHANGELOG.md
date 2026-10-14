# Changelog

Every published version of Void OSCQ, newest first. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html). A version's date is the date of its tag.

## [Unreleased]

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

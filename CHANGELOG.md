# Changelog

Every published version of OSCQuery Inspector, formerly void-oscq and published under that name up to 0.2.3, newest first. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html). A version's date is the date of its tag.

## [Unreleased]

### Changed

- **The macOS app is signed with a Developer ID and notarised by Apple.** It opens with a double click and one confirmation, where up to 0.5.1 macOS stopped it until **Open Anyway** in **Privacy & Security**. The Windows program is still unsigned.
- **The README is the front page, the manual is its own file.** The README keeps what the application is, its features, the OSCQuery it implements and the links. Installation and first launch are in `docs/install.md`, the manual in `docs/manual.md`, the tested servers and systems in `docs/compatibility.md`, building, the tools, the budgets, the release, the status and the backlog in `DEVELOPMENT.md`. Its images show the window of 0.5.1, one large and three thumbnails beside the map.

### Fixed

- **The manual describes the preset gestures of 0.5.0.** It still said one click captured into an empty slot and that each slot carried **Capture** and **Recall**.

## [0.5.1] - 2026-11-21

### Fixed

- **A setting saved twice in the same moment no longer puts back the older one.** 0.5.0 made a save overtaken by a newer one step aside, but only when the two reached the disk one after the other. When both passed their check at once, the older could still be written last, and the setting or the preset captured just before was lost at the next launch. The check and the write to disk now happen as one step.

## [0.5.0] - 2026-11-20

### Changed

- **A new application icon.** A round magnifier on a black square: a white ring, a black void, an orange core in the signal colour, and a white handle. It replaces the dark blue square and its tree, follows the application's own palette, and is drawn on a 16 unit grid whose strokes and gaps stay readable at 16 pixels. On Windows and Linux it is a plain square with no margin; on macOS it keeps the system's rounded shape, since macOS 26 shrinks an icon that leaves it onto a grey one. The Dock, the Finder, Explorer, the taskbar and the Linux menu show it once rebuilt.
- **The preset slots take the gestures of Max's `preset` object.** A **click** recalls, a **shift click** captures into any slot, and a **shift option click** empties one (option is alt on Windows and Linux). A click on a slot holding nothing now recalls nothing and says what to press instead: it used to capture there, so one gesture meant two different things depending on a state the slot showed only as a change of border. Nothing asks for a confirmation, as before.
- **A filled slot says what it holds.** Under its number it carries how many values it holds and the name of the server it was captured on, cut to the square whatever the server calls itself. Sixteen identical numbered squares could not be told apart, so a slot captured on one server and recalled onto another looked the same as any other until after it had been sent.
- **The three gestures and the scope of a capture are written under the grid.** "Click recalls. Shift click captures every writable node. Shift option click clears.", with "the pinned nodes" in place of the scope when nodes are pinned. The word Capture used to appear nowhere on screen, and what a capture would take was in the hover text of an empty slot alone.
- **The note after a capture reads properly and says why a method was skipped.** It said "12 values of methods captured" when nothing was pinned, and called a method whose type a preset cannot hold one "without a value".

### Added

- **A preset slot can be emptied.** There was no way to empty one: a slot that had ever been captured into stayed filled for the life of the config, and could only be replaced. **Clear** is in the right click menu beside **Recall** and **Capture**, as well as on the shift option click.

### Fixed

- **A setting saved twice in the same moment is no longer lost, and no longer reports a failure.** Both saves built the name of their temporary file from the config's own, so they opened the same one: the first to finish carried it away under the second, which failed and put "config not saved to ..." in the status bar in place of what the action had just done. Each save now writes its own file, and a save overtaken by a newer one steps aside instead of putting back what the newer one replaced.

## [0.4.2] - 2026-11-18

0.4.1 was tagged and built on the same day and never published: the acceptance check of a first launch found it saying "none found" again, for the reason below. Its two fixes are here, corrected.

### Fixed

- **"None found" is no longer said about a network this application may never have reached.** On macOS the system decides whether an application may look for devices on the local network, and each build of an application is a new one to it. Until it has decided, the first seconds of queries are refused outright and the ones after that are taken and dropped, with no dialog shown: nothing comes back, and nothing in the system says why. The **Discovered** panel now says which of three things it has observed. While the queries are refused it says it is waiting for permission and where to give it. While they go out with nothing answering it says that, and where to look if a server is running. Only once something has really answered does it say that none was found. A server of your own machine advertised at `localhost` is not such an answer: a blocked application still hears those, which is how a window could show one server of this Mac and none of the others.
- **Servers appear once you allow it, without quitting the app.** The permission can arrive a minute into a session, and the mDNS library the app uses doubles the gap between its own queries up to an hour, so quitting and opening the app again looked like the only cure. The app now sends a query of its own every two seconds while the system is refusing, every five seconds while nothing has answered, and every minute after the first two minutes, until something answers. Nothing is sent once a server has been heard from.
- **A server that stopped advertising no longer looks like a live one.** It stays in the list, so it can be selected and retried, and its square is now hollow: a filled square in the signal colour means a server pushing values, and the same mark cannot stand for both.

### Changed

- **An update asks for the local network permission again.** macOS treats each build of an application as a new one, so **System Settings**, **Privacy & Security**, **Local Network** lists the version you had and the one you installed. The dialog does not always appear. The README says what a first launch and an update look like, and where to look when nothing answers.

## [0.4.0] - 2026-11-17

### Changed

- **A new look, applied everywhere.** Both themes are now the application's own, built from one palette of the Structure Void design system rather than from the toolkit's defaults: a black ground in the dark theme, white in the light one, square corners, no shadows, borders of one hairline. The light theme is not the dark one subtracted from white: each of its greys is chosen so that every control and every text stands out from its ground at least as strongly as in the dark theme.
- **One colour, and it means something.** `#FF7A1A` is the only colour either theme uses, and it is kept for what is live or wants attention: a connection pushing values, a value that arrived, the count of other clients listening to a method, a failure, which also says in words what it is. Everything the window has merely observed is grey. The map's edges and the JSON tab lost their colours to this rule.
- **Typefaces of its own.** Space Grotesk for the interface and Space Mono for paths, values, flags, traffic and JSON, both under the SIL Open Font License and compiled into the binary, so a window looks the same on the three systems. Their license texts travel with every download.
- **Icons instead of letters.** Refresh, copy, save, unpin, remove a manual entry and the tree's expanders are thin drawn marks rather than text, and no icon font or icon library was added to do it.
- **Controls that read as a set.** The four tabs and the traffic log's direction filter are each one segmented bar with the chosen cell inverted; the status bar's zones are parted by thin rules; sliders are a 2 px track with a 2 px upright thumb; toggles are square; pinned nodes are parted by hairlines; the map draws its servers as square boxes whose border says the state.
- **The preset slots.** Sixteen squares, four to a row, each carrying its number. A slot holding values is bordered, the one last recalled is inverted, an empty one is dim and shows a capture mark under the pointer. A left click recalls a slot that holds values and captures into one that is empty, a shift click captures into any slot, and the right click menu names **Recall** and **Capture** in words: neither action needs the right click.
- **The repository's address.** The about box and the download links now point at `github.com/gluon/oscq-inspector`, the repository's name since it was renamed. The settings folder and the macOS bundle identifier deliberately keep the old name, so an installed copy stays the same application to the system and finds its settings again.
- **The README.** The map at the top is the application as it looks now, the servers it has really been tried with are listed with what each one does, and every view is shown before and after in `docs/images/screenshots`.

### Fixed

- **The toolbars of the JSON and Traffic tabs at a narrow window.** At the smallest window the Traffic tab's row of controls ran past the edge: the direction filter was cut in the middle of a word and Pause, Clear and Export were off screen with no way to reach them. Both rows now wrap onto further lines, as the row of tabs above them already did, and every control stays on screen at any width down to the 640 px minimum.
- **A line of the traffic log is one line.** A record was cut at the edge of the pane with no way to see the rest of it. Each record is now drawn whole on one line, and the log's own sideways scrolling reaches the type tags and the arguments.

## [0.3.0] - 2026-11-14

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

- **The map's places**: nodes are placed by forces, those connected together drawn together, spread over the map's area, and still once placed.
- **A readable map**: every server's name is always drawn at a readable size. When the map does not hold with every detail readable, it is drawn compact, a server as its name, a peer as a dot with its name, the rest on hover; when room is short an edge's label goes first, then a client's name, never a server's. **Fit** shows the whole map when it can be read, the rest is panned to otherwise. A text too small to read is not drawn.
- **Declared names**: a server is called by the name its `HOST_INFO` declares, even when entered by hand: on the map, in the server list, in the status bar and in a preset slot. Its address comes second, below on the map, first on hover elsewhere.
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

[Unreleased]: https://github.com/gluon/oscq-inspector/compare/v0.5.1...HEAD
[0.5.1]: https://github.com/gluon/oscq-inspector/compare/v0.5.0...v0.5.1
[0.5.0]: https://github.com/gluon/oscq-inspector/compare/v0.4.2...v0.5.0
[0.4.2]: https://github.com/gluon/oscq-inspector/compare/v0.4.0...v0.4.2
[0.4.0]: https://github.com/gluon/oscq-inspector/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/gluon/oscq-inspector/compare/v0.2.3...v0.3.0
[0.2.3]: https://github.com/gluon/oscq-inspector/compare/v0.2.2...v0.2.3
[0.2.2]: https://github.com/gluon/oscq-inspector/compare/v0.2.1...v0.2.2
[0.2.1]: https://github.com/gluon/oscq-inspector/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/gluon/oscq-inspector/compare/v0.1.1...v0.2.0
[0.1.1]: https://github.com/gluon/oscq-inspector/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/gluon/oscq-inspector/releases/tag/v0.1.0

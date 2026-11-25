# Release acceptance

The manual checklist run by the human before a release, on the files of the draft GitHub Release that the release tag created, not on a build from source. Its last step publishes the draft. Each step gives what to do and what must be seen. A step whose result differs is a failed release: note what was seen instead.

Three servers are available and covered: Vidvox OSCQuery Helper, Max 9, and this project's synthetic server. VDMX and an ossia based patch, named in PRD 12, are not available and are not covered. No covered server is a third party server that notifies over its WebSocket: the live update of a value changed inside a real server stays unproven, see "Known limit" at the end.

Record for the release: the tag, the CI run, the macOS version, the date, and the result of each section.

## 0. The artifact

First, with no one at the machine: `./ci/accept-artifact.sh vVERSION`, run on macOS by the workflow **Accept artifact**, from a checkout that holds the tag. It downloads the files of the tag's Release or draft and stops at the first check that fails. What it checks, each check printing what it proves: the three files, `oscq-inspector-VERSION-macos-universal.zip`, `oscq-inspector-VERSION-windows-x86_64.zip` and `oscq-inspector-VERSION-linux-x86_64.tar.gz`, and no other; the notes, the version's section of CHANGELOG.md at the tag; the macOS zip giving **OSCQuery Inspector.app**, both its version keys, `CFBundleShortVersionString` and `CFBundleVersion`, equal to the tag's version, which is what the Finder's Get Info shows; a universal binary, `arm64` and `x86_64`; `Contents/Resources/licenses` holding `LICENSE`, `LICENSE-EXCEPTION` and as many font licenses as `licenses/fonts` holds at the tag, six at 0.5.0, identical to the tag's; the application starting to its first frame and quitting with status 0 on the quit event, the one the application menu and the Dock send. Do not go on unless it ends with `ok`.

What no script here can judge is left to a person, on the Mac that gives the verdict.

1. In GitHub, **Releases**, open the draft of the tag; a draft is visible to the repository's writers only. Download `oscq-inspector-VERSION-macos-universal.zip` with the browser, unzip it, move **OSCQuery Inspector.app** to Applications. Double click it, as [install.md](install.md) says for a signed version. Quit, and open it again by a double click.
   Expected: macOS asks once whether to open an app downloaded from the internet and says Apple checked it for malicious software, never that it cannot verify the app, never that it is damaged; after **Open** the window opens; the second time it opens with no warning.
   Why by hand: Gatekeeper judges a file by the quarantine its browser download leaves on it, and answers with dialogs that wait for a person. The script downloads with `gh`, which leaves no quarantine, and the CI machine's security settings are not those of a Mac downloading from a browser.
2. Look at the app in the Finder, and at the Dock while it runs.
   Expected: in both, its own icon: a black rounded square holding a white round magnifier, its lens a white ring around an orange core. Not the generic application icon, not another application's.
   Why by hand: whether the icon drawn is the right one is a judgment of a picture, made where the Finder and the Dock draw it; no command reads back what they show, and their icon caches can show an icon the bundle no longer holds.
3. Choose **About OSCQuery Inspector** in the application menu, at the left of the menu bar.
   Expected: the application's own box, not the system's panel with a folder icon: OSCQuery Inspector, the version equal to the tag without the `v`, "Julien Bayle, Structure Void", "License GPL-3.0-or-later", and a link to github.com/gluon/oscq-inspector that opens the repository. A plain box, like no system window.
   Why by hand: the menu belongs to macOS's menu bar, which a script reaches only through accessibility permissions the CI machine does not grant, and that the menu item opens this box and not the system's panel is seen, not read from the bundle. The box's content is locked in CI by `the_about_box_draws_its_six_pieces_of_information`, on the code; this check is on the downloaded app.

Proves: the tagged CI run gives a macOS app that opens on this Mac with its Developer ID signature and its notarisation, as documented, with its icon and its own About box; with the script, its files, version, architectures, licenses and a clean start and quit. Cannot prove: the first launch on other macOS versions, or on Intel if this Mac is Apple Silicon; the Windows and Linux artifacts, which need their own systems (section 4).

## 1. Vidvox OSCQuery Helper, on the local network

Setup: OSCQuery Helper running with its `SampleDocument`, its OSC target port set (1240 in the captured setup), and an OSC monitor listening on that port to see what arrives.

1. Open OSCQuery Inspector. If macOS asks whether it may find devices on the local network, click **Allow**.
   Expected: OSCQuery Helper listed on the left within two seconds, once, even if the Mac has several interfaces.
2. Select it.
   Expected: the tree loads, 22 nodes in the status bar, which shows "connected" and the OSC destination with the Helper's target port. Under **OSC destination** in the server pane, "port advertised" and no OSC port field (an advertised port is not editable, and wins over any port set earlier for this server). Hovering "connected" says "values on refresh only".
3. Expand `/test`.
   Expected: every method has a widget, flags `w` at the right of its row: the Helper declares every node write only. `/test/my_true` and `/test/my_false` show a toggle. Names, widgets and type tags each stand in their own column, the widgets on one vertical line.
4. Drag the slider of `/test/my_float` and release it.
   Expected: the monitor receives `/test/my_float` with one `f` argument following the slider, at most one message per 8 ms while dragging, and the final value on release. The row's widget then holds the value sent, marked "sent" in its right column, and the slider stays where it was released.
5. Set `/test/my_longlong` to 42.
   Expected: the monitor receives `/test/my_longlong ,h 42`.
6. Click the toggle of `/test/my_true`; click the button of `/test/my_infinity`.
   Expected: the monitor receives a `T` or `F` message, then an `I` message.
7. Select `/test`, open the **JSON** tab, click **Copy** and paste the text somewhere.
   Expected: the document of `/test` exactly as the Helper sent it, `\/` escapes included.
8. Open the **Traffic** tab.
   Expected: every message of steps 4 to 6, direction `out`, with path, type tags and arguments as the monitor received them. **in** shows none of them, **out** all of them. **File ▸ Export ▸ Traffic…** opens the system's save dialog and writes a text file, its path shown in the status bar.

Proves: discovery by Bonjour on the LAN, `HOST_INFO` with an advertised port, sending OSC to a real server with the declared types, the write only display, the JSON and traffic tabs. Cannot prove: values read from the server (the Helper serves none), the WebSocket (the Helper lists no `LISTEN` and has no value to push, see "Known limit").

## 2. Max 9, on the same Mac

Setup: Max 9 running `tests/fixtures/max/test.maxpat`, which serves OSCQuery on `localhost:30339`. Close OSCQuery Helper.

1. Click **+ manual**, enter `localhost:30339`, connect.
   Expected: the tree loads with `/test/param/gain`, `/test/param/freq` and `/test/param/fader`, each holding ten nodes (`longname`, `min`, `max`, `raw`, `normalized`...), with their values. Under **OSC destination** in the server pane, "port inferred from the HTTP port" and an OSC port field; hovering the OSC destination in the status bar says "HOST_INFO ignored by the server, defaults used". No error.
2. Wait a few seconds after the tree loads.
   Expected: the status bar says "connected, silent", and hovering it "WebSocket connected, no update received yet: values on refresh only". Never "live".
3. Change `/test/param/fader/normalized`, a read write node, with its widget, with nothing else set.
   Expected: the status bar says "OSC localhost:30339 UDP", the server pane "port inferred from the HTTP port", and the fader in Max does not move: Max receives OSC on 30338, see docs/protocol-notes.md section 12.
4. In the server pane, under **OSC destination**, type 30338 in the OSC port field and click **Set OSC port**. Send again.
   Expected: "OSC localhost:30338 UDP" in the status bar, "port set by the user" in the server pane, and the fader moves in Max.
5. Change `/test/param/fader/normalized` with its widget again.
   Expected: the fader moves in Max. The row shows the value sent until a value is read.
6. Select `/test/param/fader/normalized` and press F5.
   Expected: the value read from Max in the widget, no longer marked "sent".
7. Move the fader in Max, and do not touch OSCQuery Inspector.
   Expected: the value shown does not change by itself, since Max sends no notification; F5 shows the new value.
8. Quit OSCQuery Inspector and open it again.
   Expected: the server reconnects with "OSC localhost:30338 UDP" and "port set by the user"; sends reach Max. **Infer OSC port** goes back to 30339, "port inferred from the HTTP port"; set 30338 again before going on.
9. Close the patch in Max, then click the server on the left to reconnect.
   Expected: the status bar says the connection failed once, with the system's reason (for example "Connection refused"). The tree stays; its read write nodes still show `rw`, have no widget and say "cannot send", the reason on hover. Never "read only".
10. Open the patch again and click the server.
   Expected: connected, the widgets back, no error left in the status bar.
11. **File ▸ Save Project…** (Cmd S), save. Remove the Max entry with its **x**, then **File ▸ Open Project…** (Cmd O) on the file saved, and select the entry.
   Expected: on macOS the File menu is in the menu bar and the dialogs are the system's own sheets. The Max entry is back, with "OSC localhost:30338 UDP" and "port set by the user"; the status bar says where the project replaced is kept, in `exports/`.

Proves: a server that ignores the `HOST_INFO` query is used with the proposal's defaults and no error, reading values from a real server, sending to a real server on the port set by the user where the inferred one is wrong, that port kept across a restart and in a project, the honest display of a WebSocket that accepts and stays silent, the File menu and its dialogs on the system. Cannot prove: live values (Max does not notify), discovery (Max does not advertise and listens on 127.0.0.1 only).

## 3. Synthetic server, on the same Mac

Setup, from a clone of the repository at the release tag:

```
cargo run -p oscq-core --example synthetic_server --release -- --port 5678 --stream 2
```

1. In OSCQuery Inspector, **+ manual**, `127.0.0.1:5678`, connect.
   Expected: 5050 nodes in the status bar, the tree shown at once and smooth to scroll.
2. Expand `/g0`.
   Expected: within a second, "live" in the status bar; the values of the visible rows change twice a second by themselves, each row its own value. The server's terminal shows the number of listens, close to the number of visible rows.
3. Scroll `/g0` down, then collapse it.
   Expected: the server's listen count follows the rows on screen, and drops to 0 when collapsed.
4. Stop the server with Ctrl C.
   Expected: the status bar shows "connected, socket lost" and the retry delay, growing on each attempt, up to 15 s.
5. Start the server again with the same command.
   Expected: back to "live" within the retry delay; with `/g0` expanded, values move again, without touching the app.
6. With the pointer outside the window and nothing changing, watch the CPU of OSCQuery Inspector in Activity Monitor for ten seconds.
   Expected: about 0 %.

Proves: the nominal WebSocket path end to end, listen and ignore following the rows on screen, reconnection with backoff and listens sent again, idle with no repaint. Cannot prove: interoperability with a third party server: the synthetic server is this project's own.

## 3b. A HOST_INFO whose warning the window must name, synthetic server

Nothing from the sections above is needed here, and nothing else must be listening on port 5690. From a clone of the repository at the release tag, in a terminal of its own:

```
cargo run -p oscq-core --example synthetic_server --release -- --port 5690 --doc crates/oscq-core/examples/one-method.json --host-info crates/oscq-core/examples/host-info-empty-ws-ip.json
```

Expected in that terminal: a line beginning `http://127.0.0.1:5690`, and under it `HOST_INFO from crates/oscq-core/examples/host-info-empty-ws-ip.json, served as it is: --name does not reach it`. That file declares `WS_IP` as an empty string, which names no host. A REAPER server was found doing it, and until 0.6.0 the application built `ws://:5690/` from it, which nothing can connect to, so no `LISTEN` was ever sent.

1. In OSCQuery Inspector, **+ manual**, `127.0.0.1:5690`, connect.
   Expected: the node count of the status bar reads exactly `1 node, 1 warning`, both in the singular: not `1 nodes`, not `1 warnings`.
2. Leave the pointer still on that `1 node, 1 warning` until its tooltip appears.
   Expected: one line, `HOST_INFO WS_IP: "" treated as absent`: the attribute, the value the server sent for it, and that it counts as absent. The empty pair of quotes is in it, so what is read is the value itself and not a word standing for it.
3. Read the state at the left of the status bar, and the OSC destination beside it.
   Expected: `live` or `connected, silent`, and in no case `values on refresh only`. The empty `WS_IP` counted as absent, the WebSocket went to the HTTP host and opened. The destination says the port was inferred from the HTTP port, the file declaring no `OSC_PORT`.
4. Stop the server with Ctrl C, wait for the status bar to say the socket was lost, then start it again with the same command.
   Expected: the warning comes back with the connection, still counted once and not twice.

Why by hand: a tooltip is drawn by a pointer left still, and whether its line says the attribute and the value a person can act on is a judgment of what the screen shows. The headless tests of `app.rs` lock the same two strings on the code, `a_counted_warning_is_named_on_hover_with_its_attribute_and_its_value` reading the glyphs laid out; this check is on the built application.

Proves: an unusable `HOST_INFO` attribute is named on screen with the value received, in the singular, on the real binary and seen by a person; and that an empty `WS_IP` no longer keeps the WebSocket shut. Cannot prove: that a real server sending the same is read the same way. The synthetic server serves the file it was handed, byte for byte, and REAPER, the server this was found on, is not among the three covered here.

## 3c. The same warning against a real server, ossia-prism in Max on the same Mac

This is the check 3b states it cannot make. It needs Max with the `ossia-prism` package at `2.1.0+prism.18`, the version that serves the empty attribute; the package's `package-info.json` names the one installed. `2.1.0+prism.19` does not reproduce it, having been changed to serve `127.0.0.1`, which its authors report and this repository has not verified. Nothing else must be listening on port 5678.

In Max, open `distant_device.maxpat` from the package's help files and click its `expose oscquery` message box. The device holds one model and two writable floats, `foo` and `bar`, so four nodes with the root. The status bar leaves the root out of its count, which is why the number to read below is three.

Worth reading once before the window is opened, the point of this check being that the document is nobody's fixture here:

```
curl -s 'http://127.0.0.1:5678/?HOST_INFO'
```

Expected in it: `"WS_IP":""`, with `"WS_PORT":5678`, `"OSC_PORT":9999`, `LISTEN` among the extensions, and no `OSC_IP` key at all.

1. In OSCQuery Inspector, **+ manual**, `127.0.0.1:5678`, connect.
   Expected: the status bar reads exactly `3 nodes, 1 warning`.
2. Leave the pointer still on that count until its tooltip appears.
   Expected: `HOST_INFO WS_IP: "" treated as absent`, the same line as 3b, from a server nobody here wrote.
3. Read the state at the left of the status bar.
   Expected: `CONNECTED, SILENT`, and in no case `values on refresh only`. Silent rather than live is right: the device pushes nothing until one of its two parameters moves.

Why by hand: the reason 3b gives, and one more. A fixture proves the code reads what this repository wrote; only a server written elsewhere proves it reads what the world sends.

Proves: that the empty `WS_IP` of a real server is counted as absent and named on screen with the value received, and that the WebSocket address the fallback chooses is where that server answers, a handshake on `http://127.0.0.1:5678/` being answered `101 Switching Protocols` by `Boost.Beast/359`. Cannot prove: that two implementations agree, the empty field coming from one code path, the beast server's `get_local_ip`, seen in two deployments and not in two libraries; anything about `2.1.0+prism.19`, which was not run here; and nothing about the release artifact, this reading having been taken on a debug build wrapped in a minimal bundle.

## 4. Windows and Linux, when available

Not required for the macOS release. On each system available:

1. Download its file from the draft, follow the first launch procedure of [install.md](install.md) for that system.
   Expected: the window opens; on Windows, no console window. The executable in Explorer, the window and the taskbar show the application's icon; on Linux, the icon in the application menu once installed as [install.md](install.md) says.
2. Run section 3 with the synthetic server on that machine, or section 1 if a Helper runs on the LAN.
   Expected: the same results.

Proves: the artifacts of that system. Cannot prove: the systems not run.

## 5. Interface: server list, pinned nodes, presets, traffic

Setup: the synthetic server of section 3 running, Max 9 with its patch as in section 2, both entered with **+ manual**.

1. Look at the window with the synthetic server selected and `/g0` expanded.
   Expected: in the tree, three columns: names, widgets on one vertical line, type tags and flags right aligned; a thin vertical guide per level of indentation. Paths, values and type tags in Space Mono, the rest in Space Grotesk. The status bar is one line, its zones parted by thin rules: state, server, OSC destination, node count, then the last message. Square corners everywhere, no shadow, borders of one hairline. The only colour anywhere is `#FF7A1A`, and only on what is live or wants attention: the state dot and its word while values are being pushed, the "sent" mark, a listener badge, a failure. Everything else is grey.
2. Switch **Theme** to light, then dark, then system; quit and open again.
   Expected: each one applied at once; system follows the setting of macOS; the last choice kept after the restart.
3. In the server pane, look at the **Discovered** and **Manual** sections.
   Expected: the manual entries under **Manual**, each with a cross at the end of its row; a discovered server has none while it is advertised, and one once it is stale, with **Clear stale** under the discovered servers.
4. Right click `/g0/p0`, **Pin**; the same for `/g0/p1` and `/g0/p2`. Collapse `/g0`, open the **JSON** tab.
   Expected: the three nodes in **Pinned** on the right, with their widgets, whatever the tree or the tab shows. Moving a pinned widget sends, as in the tree.
5. Set the three pinned values to 0.1, 0.2, 0.3. The preset section is folded: click **PRESETS** to open it.
   Expected: sixteen square slots, four to a row, each carrying its number and nothing else, all of them dim since none holds anything. Under the grid: "Click recalls. Shift click captures the pinned nodes. Shift option click clears." (alt in place of option on Windows and Linux). Rest the pointer on slot 01: "Slot 01: empty" and "Shift click to store the pinned nodes of this server."
   Then click slot 01, which is empty.
   Expected: nothing sent, and the note "Slot 01 is empty: shift click to capture into it".
   Then shift click slot 01.
   Expected: the note "Slot 01: 3 pinned methods captured". Slot 01 is bordered in the brightest ink on the control surface, and under its number it carries 3 and the synthetic server's name, told apart from the fifteen dim ones at a glance.
6. Move the three values elsewhere, then click slot 01, which now holds values.
   Expected: the three widgets back to 0.1, 0.2, 0.3 at once, with no confirmation, and slot 01 drawn inverted, ink and ground swapped, as the slot last recalled. The **Traffic** tab shows one line, its time then `out preset 01  (3 messages)`; clicking it lists the three messages.
   Then shift click slot 01 again.
   Expected: it captures again rather than recalling, the note saying so. Shift click slot 02, then shift option click it.
   Expected: slot 02 filled, then empty and dim again, the note "Slot 02 cleared". Right click any slot.
   Expected: a menu naming **Recall**, **Capture** and **Clear** in words, **Recall** and **Clear** greyed on an empty slot.
7. In **Traffic**, click **IN**, then **OUT**, then **ALL**, in the segmented bar beside the filter.
   Expected: **IN** shows only values received, **OUT** only what was sent, **ALL** both; the chosen one is drawn inverted. Narrow the window to its smallest, 640 points.
   Expected: the tab's own row of controls wraps onto further lines and every one of them stays on screen, the filter, the bar, **PAUSE**, **CLEAR** and the count; each record of the log stays on one line, the rest of it reached by scrolling sideways.
8. Right click a node, **Copy path**, and paste it somewhere.
   Expected: its full path.
   Then click the cross at the end of the row of `/g0/p2` in **Pinned**.
   Expected: `/g0/p2` leaves the panel; pin it again from the tree.
   Type `p1` in the filter.
   Expected: each hit named first, `p1  /g0`, `p10  /g0`, `p100  /g1`, the containers after the name; the full path on hover.
   On a window of 1060 points or more (2120 pixels on a Retina screen), the toolbar holds on one line.
   Type `fader/` in the filter, on Max 9's tree.
   Expected: the hits by their names alone, no container path: they are all in one container.
   Check **Writable** in the toolbar, clear the filter.
   Expected: Max 9's tree without `longname`, `shortname`, `type`, `min`, `max` and the other read only metadata; the status bar says how many nodes are hidden. Quit and open again: still checked.
   Switch **Theme** to light.
   Expected: the same system inverted, a white ground and black text, the signal colour unchanged: an editable value stands out from a read only one as clearly as in the dark theme.
9. Quit and open again, select the synthetic server.
   Expected: the three pinned nodes and slot 01 still there.
10. Select Max 9, click slot 01.
   Expected: nothing sent, the note says "Slot 01 recalled: 0 sent, 3 not found on this server (captured on ...)". Nothing blocks.
11. Click the cross of Max 9's manual entry, then **+ MANUAL** `localhost:30339` again.
   Expected: the entry leaves the list at once; added again, its OSC port is inferred from the HTTP port, 30339: the port set earlier was forgotten with the entry.

Proves: the interface rework seen by a person, the server list removal, the pinned panel across tabs and restarts, a capture, recall and clear cycle on the same server and a recall on another one, with the gestures written under the grid, the traffic direction filter. Cannot prove: anything beyond this Mac and these two servers.

## 6. Peers and the map

Setup: two applications that implement the `PEERS` and `CLIENT_NAME` extension and declare it in their `HOST_INFO`, called A and B here, A connected to B as a client of its WebSocket, on this Mac or the LAN; and Max 9 as in section 2, which declares nothing. The synthetic server cannot stand for A: it is never itself a client of another server. Enter all three with **+ manual**, or let discovery find them, and select each once so that the app connects to it.

1. Open the **Map** tab.
   Expected: one node per server in the list, the three included. An arrow from A to B, its label the number of values A sent; the node of A titled A, not "unnamed client". A node "OSCQuery Inspector", "this application", with an arrow to A and one to B. Max 9 drawn, "does not publish its peers", no arrow to or from it. Above the map: read every 2 s while this tab is shown, not live. Under A and B: read 0 or 1 s ago, never more than 2.
2. In A, make it listen to one method of B. Select B, open the **Tree** tab, find that method.
   Expected: on that method's row, in the metadata column, a badge "1"; on hover, A's name. No badge on its container, its siblings or its children. The line above the tree: listeners as of a time of day a few seconds ago.
3. In A, stop listening to it. Press **Refresh**.
   Expected: the badge leaves.
4. Select Max 9, **Tree** tab.
   Expected: no badge anywhere; the line above the tree says the listeners are not published by this server.
5. Back on **Map**, quit A.
   Expected: within a reading, A's arrow to B leaves; B no longer lists it.
6. With the map shown, the pointer outside the window, watch the CPU of OSCQuery Inspector in Activity Monitor for ten seconds; then the same on the **Tree** tab.
   Expected: about 0 % on both.

Proves: the extension read from two real implementations, the edge between them, the badge on exactly the methods a peer listens to and its leaving, a server without the extension drawn with neither edge nor badge. Cannot prove: implementations other than the two run, and a setup larger than the one on the table.

The map handled by hand, since 0.3.0. Same setup; A entered with **+ manual** by its address, `host:port`, so that it is listed under the address until its `HOST_INFO` is read.

7. Select A once. Look at the server list, the status bar and the **Map** tab.
   Expected: A called by the `NAME` its `HOST_INFO` declares in all three, not by its address. Its address on the line below its title on the map, and first on hover of its row in the list and of the status bar's server zone.
   Proves: what the server declares wins over the name it was entered under, everywhere the window names it, on a real implementation.
8. On **Map**, drag B's node to an empty part of the map and let go. Take the hand off the mouse for ten seconds.
   Expected: B where it was put; the other nodes where they were; nothing moves afterwards. B is drawn once, its hover only adding what its box does not show.
   Proves: a position set by hand wins over the placement, moves that node alone, and the map stays still at rest after a drag; no node is drawn twice with the pointer resting on it.
9. Quit OSCQuery Inspector and open it again; open **Map**.
   Expected: B at the place it was put.
   Proves: the position is written to the config and read back at the next launch, on the machine.
10. Press **Release**, beside **Fit**.
    Expected: B placed again by the map, with the others; **Release** greyed until a node is moved again. Quit and reopen: B is not where it was put.
    Proves: Release lets go of every position set by hand, the config's included.
11. Click a node that has edges, A for instance.
    Expected: A, its edges and the nodes at their other ends at full colour, everything else dimmed, at once, with no fade. Click A again: everything at full colour. Click A, then press Escape: the same.
    Proves: lighting a node and putting it out by a second click or by Escape, with no animation.
12. Pin one method of B from its **Tree** tab, select A in the list, open **Map**. Click B's node. Then double click A's node. Then click the node "OSCQuery Inspector".
    Expected: after the click, B selected in the server list and in the status bar, the pinned panel on the right showing B's pinned methods with their widgets, usable from the map, which stays shown. After the double click, A selected and its **Tree** tab open at its root. After the click on "OSCQuery Inspector", the selection unchanged: it is not a server.
    Proves: one selection for the whole window, set from the map as from the list; a click on a node that is no known server lights it and selects nothing.
13. Drag a node, Max 9's for instance, onto the middle of the edge from A to B and let go.
    Expected: the edge's label, its count of values, moves along its edge, or is not drawn; it never covers the box of a node or its title.
    Proves: an edge's label never covers a node, on the machine's fonts and display.

Proves, 7 to 13: the map handled by hand on the machine that gives the verdict. Cannot prove: more than the setup on the table, and a pointer other than the one used.

## 6b. The local network permission on a first launch, macOS

Why: macOS decides whether an application may reach the local network per build of it, not per
bundle identifier, so every version asks again. Until it has decided, the first seconds of
queries are refused outright and the ones after that are taken and dropped, with no dialog
guaranteed to appear. Nothing can be found in that state, and the window must say what it has
observed rather than that nothing is there. This section is the check 0.4.1 failed.

Setup: a Mac running macOS 15 or later, the version under test never launched on this Mac
before, and two servers:

- one advertised on the Mac's Wi-Fi or Ethernet address, from this Mac or from another machine:
  `cargo run -p oscq-core --example synthetic_server --release -- --port 5678` of this
  repository, or any OSCQuery server on the LAN;
- one registered with the system's own responder, `dns-sd -R lo-only _oscjson._tcp local. 9999`
  in a terminal. The responder announces it on every interface, loopback included, and an
  application the system is keeping off the local network hears the loopback copy of it: that
  copy carries loopback addresses only, which is what makes the two servers tell the states
  apart.

`dns-sd -B _oscjson._tcp local.` in a terminal lists both, and is the ground truth each step is
read against.

1. Open the downloaded application for the first time and look at the **Discovered** panel
   before answering any dialog macOS shows.
   Expected: "Waiting for permission to search the network.", in the signal colour, and under
   it where to allow it. Nowhere does it say "none found", and nowhere does it say that nothing
   has answered: nothing has been asked of the network yet.
2. Wait ten seconds, still without answering.
   Expected: either servers appear, the permission having been granted, or the panel now says
   "Searching since HH:MM. Nothing has answered.", with where to look and the manual entry
   line. It must not say "none found" while the terminal lists a server.
   Why this step: the refusal stops after a few seconds whether or not the permission was
   granted, and this is the state the window used to call "none found".
3. Click **Allow** if a dialog is showing, or, if none appeared, turn the entry for this version
   on in **System Settings**, **Privacy & Security**, **Local Network**. Do not quit the
   application.
   Expected: the server advertised on the real address appears in **Discovered** within a
   minute, with no relaunch, and the panel's lines about waiting or nothing having answered are
   gone. The log, with `RUST_LOG=oscq_inspector=info`, has "an answer arrived from the local
   network".
4. While the permission is on, look at the loopback only server.
   Expected: it is listed too. Both are listed: the loopback one proves nothing about the
   network, but it is a server and it is shown.
5. Open **System Settings**, **Privacy & Security**, **Local Network**.
   Expected: an entry for this version. If an earlier version was installed, its entry is there
   too, under the same name: one entry per build, which is the system's doing and not the
   application's.
6. Stop the server on the real address, and wait.
   Expected: its row stays in the list, drawn dim with a hollow square, and the status bar says
   "stale" when it is selected: a row no longer advertised never carries the filled square of a
   server pushing values. The panel then says "Searching since HH:MM, none found" once the list
   is empty, which is allowed here because something has answered during this session.
7. If a build the system has never seen is at hand and its permission is off or undecided, open
   it with the `dns-sd` server running.
   Expected: that server is listed, with a `127.0.0.1` address on hover, while the panel still
   says that nothing has answered, and the server on the real address is not listed. That is the
   state of 2026-11-18, and the two together are correct: a server was found, and the network
   has not answered.

Proves: that the window says what it has observed and no more (CLAUDE.md law 1), including the
difference between a send the system took and an answer from the network; that access granted
during a session is acted on without a relaunch; and that a server no longer advertised cannot
be read as a live one. Cannot prove: what happens when the permission is denied for a long
time, which no Mac here would hold a process in, and whether a dialog appears at all, which the
system decides; see PITFALLS.md.

## 7. Publication

Last, and only when every section run above gave what was expected.

1. In GitHub, **Releases**, open the draft of the tag, **Edit**, then **Publish release**.
   Expected: the release listed on the Releases page, marked Latest, with its three files; the README's download link leads to it.

A section that failed: leave the draft unpublished and note what was seen. The fix goes into a new version, with its own tag and its own draft.

Proves: what visitors download is what passed this checklist. Cannot prove: anything this checklist did not run.

## Known limit: WebSocket interoperability with a third party server

Not proven by this checklist: that OSCQuery Inspector receives the notifications of a third party OSCQuery server over its WebSocket, values changed inside the server appearing without user action, and `PATH_*` commands reloading the tree. The only server on which the notification path is proven is this project's synthetic server (section 3), written from this project's own reading of the proposal: it shows the client agrees with itself, not with anyone else.

The two third party servers tested do not notify:

| Server | WebSocket, as observed | What it does not do |
| --- | --- | --- |
| Vidvox OSCQuery Helper | Its `HOST_INFO` (`tests/fixtures/helper-hostinfo.json`) gives no `WS_IP` or `WS_PORT` and lists `PATH_CHANGED` but not `LISTEN`; `LISTEN` is commented out in its source (docs/protocol-notes.md section 4). Every node is write only, `ACCESS` 2, and `?VALUE` answers `{}`. Whether it accepts a socket on its HTTP port was not observed | No `LISTEN`, so no value frames; no value of its own to push |
| Max 9 | Accepts a WebSocket on its HTTP port 30339 and accepts `LISTEN` (`/test/param/fader/raw`, checked with `websocat`, docs/protocol-notes.md section 12) | Sends nothing on the socket: no binary value frame when a parameter moves, no `PATH_*` command |

Untested against a real server, in consequence: binary OSC value frames as a third party encodes them (bundles, type tags, timetags); `PATH_ADDED`, `PATH_REMOVED`, `PATH_RENAMED` and `PATH_CHANGED` as a third party sends them; `LISTEN` and `IGNORE` honoured by a server; the reconnection after a third party server drops its socket.

To lift it, one server that notifies over its WebSocket and is not this project's, for example VDMX or an ossia based patch, both named in PRD 12; that they notify is what their makers state, not something checked here. Then:

1. Connect to it and expand a container: the status bar says "live" only once the server has sent a frame.
2. Change a value inside the server, without touching OSCQuery Inspector: the row updates, read, not marked "sent"; the Traffic tab logs it as `in`.
3. Add or remove a node inside the server: the tree follows without a refresh.
4. Capture the frames with `websocat` while doing 2 and 3, and commit them under `tests/fixtures` with their provenance, so that the client's decoding of that server is tested in CI as the Helper and Max documents are.

Until then the limit stands, and no release note may say that live values work with third party servers.

## Acceptance record

What was proven, by whom and against what, milestone after milestone, as the README's status section recorded it until 2026-10-14. Kept word for word.

- M0, protocol groundwork: done, merged.
- M1, core client: done, merged. Proven by the human on 2026-10-09: `oscq_probe` printed the Helper's 22 node tree and a value sent to `/test/my_float` reached the Helper's target.
- M2, discovery: done, merged. Proven on the human's LAN on 2026-10-09: one entry in 0.18 s, lo0 and en0 merged, LAN IPv4 promoted.
- M3, application: done, merged. Tested by the human on the Mac against OSCQuery Helper.
- M4, WebSocket and performance: done, merged. Every PRD 10 budget met on the human's Mac or the macOS CI machine, see docs/decisions.md. Values pushed by the synthetic server seen live on the Mac; Max 9 accepts the WebSocket and sends nothing, shown as open and silent.
- M5, release: done, merged, tag `v0.1.0`. The tagged run built the three artifacts; the macOS app was installed, opened and connected to Max 9 by the human.
- M5 polish: done, merged. The OSC port setting checked by the human on the Mac against Max 9: sends reach 30338 once set, kept across a restart. The loss and return of a server, the system's cause in the error and the declared access kept were checked there too.
- v0.1.1: done, merged, tag `v0.1.1`. The tagged run 38269010531 built the three artifacts; on the human's Mac the app installed, showed its icon and 0.1.1 in the Finder, connected to Max 9 with the OSC port read from the config, and its sends moved the fader.
- Cleanup: done, merged. A reload that leaves the rows on screen is locked to send no `LISTEN` or `IGNORE`; the PRD says the repository is public; docs/acceptance.md states the known limit of WebSocket interoperability.
- M6, interface rework, server list, pinned nodes, presets, traffic filter: done, merged. Checked by the human on the Mac: the three real binary tests pass, cold start 91.5 to 105.7 ms, memory 97.8 MB, no repaint at rest. Headless tests lock the removal of a manual entry with its OSC port, the pinned panel across tabs and per server, a capture and recall on the same server and on another, and the traffic direction filter. The look waits on the human, docs/acceptance.md section 5. After the human's review: the toolbar, Refresh included, on screen at any window width; the pane handles stop dead at their limits; the narrow tree keeps the widget and gives up the name, then the metadata; the discovery line says since when it searches; the about box names the author, Structure Void, the license and the repository. License GPL-3.0-or-later, with an additional permission for the bundled fonts, after the dependency audit recorded in docs/decisions.md; every release artifact carries the license texts.
- About fix: done, merged. On macOS the application menu's About item opened the system's panel, empty outside a bundle; it now opens the application's own about box, whose six pieces of information a test checks one by one on screen. Checked by the human on the Mac: the six shown by both paths, the menu and the button.
- v0.2.0: done, merged, tagged and released; the three artifacts complete with their license texts, installed on the human's Mac.
- v0.2.1: done, merged, tagged. Checked by the human on the Mac: the toolbar on one line, the unpin button, Recall and Capture apart, a held slot seen.
- v0.2.2: done, merged, tagged. The filter path only on ambiguity, Writable only with its count, the folding preset section, the toned light theme. Checked by the human on the Mac: Max 9's tree from 35 nodes to 6 with the count, no path without ambiguity, the presets folded by default, both settings kept, the light theme.
- Toolbar label: done, merged. The check box reads Writable, and the toolbar holds on one line from 1060 points, a threshold measured in ten point steps: 980 without the box, 1060 with Writable, 1090 with Writable only. A test locks 1060 and fails with the old label.
- Public documentation: done, merged. The README addresses a user before a builder, names the three servers it was tested with and what they do not do, and says how each system is checked; CHANGELOG.md and CONTRIBUTING.md added; a tag now creates a draft Release holding the three files, published by hand once this checklist has passed; the Windows release build unzips its executable and starts it, so a Windows binary was started for the first time, first frame at 547 ms in CI.
- v0.2.3: done, merged, tagged, released. The tagged run built the three artifacts and created the draft, published by hand. Checked by the human on the Mac: the macOS download from the published Release installed, its about box shows 0.2.3, it connects to Max 9 with the OSC port kept from the config, and Writable leaves 6 of 35 nodes with its count. The artifact carries the GPL, its additional permission and the four font licenses under `Contents/Resources/licenses`.
- CLAUDE.md made public: done, merged. It states the laws the application is held to, each pointing into the PRD, and no longer carries the internal working conventions. Nothing in the tracked files names a private repository or infrastructure; the history keeps those names, by the ruling recorded in docs/decisions.md.
- v0.5.0: tagged and released on 2026-11-20. The tagged run built the three artifacts and the draft. Accept artifact passed on the draft, run 44772959196, once `ci/accept-artifact.sh` counted the font licenses the tag holds: it required four, and stopped every release since the two Space typefaces were bundled. Checked by the human on the Mac, section 0: the draft's macOS zip, downloaded with `gh` and given a quarantine attribute by hand in place of a browser download, reported as unverified, never as damaged, opened through **Open Anyway**, then with no warning; the magnifier icon in the Finder and the Dock; the application's own About box, 0.5.0. Not run for this release: sections 1 to 6b, the preset gestures of section 5 included, which the headless tests of `app.rs` cover on the code but no person has seen on this build.
- v0.6.0: tagged on 2026-11-23, the first macOS app signed with a Developer ID and notarised. The Release run 45156624642 failed four times in `macos-sign` before passing at its fifth attempt, the secrets and the developer agreement at fault and not the workflow (PITFALLS.md, macOS signing); notarisation Accepted. Accept artifact passed on the draft, run 45234573661. Checked by the human on the Mac on 2026-11-23, the macOS version not noted: the draft's macOS zip, downloaded with `gh` and given a quarantine attribute by hand, `spctl` accepted with "source=Notarized Developer ID", `stapler validate` worked, the app opened; the magnifier icon in the Dock; the application's own About box, 0.6.0. Section 6b: after `tccutil reset LocalNetwork`, the waiting line, then the servers once the permission was given, `lo-only` among them. The File menu with the synthetic server in place of Max 9: a project saved with a manual entry and a pinned node, the entry removed, the project opened, the entry and the pin back, the project replaced kept in `exports/`; the export and import of a server list, a document, the traffic and the presets checked on the branch build the same day. Section 3: live values, the loss and return of the server, 0.0 to 0.2 percent of a core at rest, 1.7 to 3.2 percent with one pinned value pushed twice a second. Not run for this release: section 1 (no Helper running), section 2 (the Max 9 test patch served nothing on 30339 that evening, cause not looked into), sections 4, 5 and 6.
- Empty `WS_IP` against a real server: checked by the human on the Mac on 2026-11-24, section 3c, `ossia-prism 2.1.0+prism.18` in Max serving `distant_device` on 5678. The status bar read `3 nodes, 1 warning`, its tooltip `HOST_INFO WS_IP: "" treated as absent`, the state `CONNECTED, SILENT`. Read on a debug build of `38eec5a` wrapped in a minimal bundle, not on a release artifact. It closes what 3b says it cannot prove: until then the empty attribute had only been met on a REAPER server, and no real server had been put in front of the window.
- v0.7.0: tagged on 2026-11-25 at `224ef23`, the merge of `void/release-0.6.1` into main. The Release run 45693608000 passed, the macOS Developer ID signature and the notarisation included, and the draft holds the three files. `ci/accept-artifact.sh v0.7.0` passed on the draft, run on the Mac: the three files, the notes, version 0.7.0 on both keys, universal `x86_64` and `arm64`, 8 license texts, first frame 189.6 ms, clean quit. Checked by the human on the Mac on 2026-11-25, macOS 15.7.7 build 24G720, Apple M1 Max. Section 0: the draft's macOS zip downloaded with `gh` and given `com.apple.quarantine` by hand on the zip and on the app, `-r`, as for v0.6.0, unzipped with `ditto` and moved to `/Applications`; `spctl` accepted with `source=Notarized Developer ID` and `origin=Developer ID Application: Julien Bayle (8UX77EF9ZR)`, `stapler validate` worked, `codesign --verify --deep --strict` valid on disk and satisfying its Designated Requirement, `CFBundleShortVersionString` 0.7.0; Gatekeeper, the Dock icon and the About box as expected. The quarantine attribute was kept, so macOS ran the app translocated out of `/private/var/folders`: the same signed bundle under another path. Section 3b: the synthetic server on 5690 serving `host-info-empty-ws-ip.json`; `1 node, 1 warning` in the singular, the tooltip `HOST_INFO WS_IP: "" treated as absent` carrying the value as received, `CONNECTED, SILENT`, and `127.0.0.1:5690 UDP` with `port inferred from the HTTP port`. After the server was stopped and started again the warning came back counted once, and the socket reopened: the server's own log registered the listen a second time, and `lsof` showed an established pair between the window and it. The orange `values on refresh only` read during that restart was `WsState::Lost`, the retrying state, whose distinguishing half a narrowed window had cut off; see the backlog of [DEVELOPMENT.md](../DEVELOPMENT.md). Section 3c: `ossia-prism 2.1.0+prism.18` restored in Max from the copy retired on 2026-11-24, and `2.1.0+prism.20` put back afterwards, both verified; `HOST_INFO` serving `"WS_IP":""` with `WS_PORT` 5678, `OSC_PORT` 9999, `LISTEN` true and no `OSC_IP`; the window reading `3 nodes, 1 warning`, the same tooltip as 3b, `CONNECTED, SILENT`, and `OSC 127.0.0.1:9999 UDP` with the port advertised. Unlike the reading of 2026-11-24 this one is on the release artifact and not on a debug build in a minimal bundle. Not run for this release: sections 1, 2, 3, 4, 5, 6 and 6b. Discovery is not among them but was exercised separately before publishing, the window's `Searching since 22:03. Nothing has answered.` having left open whether anything was announcing at that hour: a Bonjour record was registered with macOS's own `dns-sd` as `oscqi-discovery-check._oscjson._tcp.local.` on port 5690, where a synthetic server of 12 methods was answering, and the human saw it listed in the discovered pane. So the browse of `_oscjson._tcp` works on this build and the local network permission is granted; whether it was already granted at 22:03 or given during that check is not established, and the emptiness at 22:03 is explained by nothing announcing. Not proven by it: that a third party server's record is read correctly, which is section 1 with the Vidvox Helper and was not run; the record came from `dns-sd` and not from a server announcing itself.

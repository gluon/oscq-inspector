# Release acceptance

The manual checklist run by the human before a release, on the artifacts of the release tag, not on a build from source. Each step gives what to do and what must be seen. A step whose result differs is a failed release: note what was seen instead.

Three servers are available and covered: Vidvox OSCQuery Helper, Max 9, and this project's synthetic server. VDMX and an ossia based patch, named in PRD 12, are not available and are not covered. No covered server is a third party server that notifies over its WebSocket: the live update of a value changed inside a real server stays unproven, see "Known limit" at the end.

Record for the release: the tag, the CI run, the macOS version, the date, and the result of each section.

## 0. The artifact

1. In GitHub, Actions, workflow **Release**, open the run of the tag. Under **Artifacts**, download `void-oscq-VERSION-macos-universal`.
   Expected: one artifact per system, macOS, Windows, Linux, each named with the tag's version.
2. Unzip the download, then the zip inside it. Move **Void OSCQ.app** to Applications.
   Expected: an application named Void OSCQ, with its own icon in the Finder: a dark blue rounded square holding a white tree, a root and two leaves, one leaf amber. Not the generic application icon.
3. Double click it, and follow the README's macOS first launch procedure: **Privacy & Security**, **Open Anyway**.
   Expected: macOS reports an app it cannot verify, never an app that is damaged; after **Open Anyway** the window opens.
4. Quit and open it again by a double click.
   Expected: it opens with no warning.
5. Click **About**, at the bottom of the server pane.
   Expected: version equal to the tag, without the `v`.
6. While it runs, look at the Dock.
   Expected: the same icon as in the Finder, not the generic one and not another application's.
7. Quit it. In the Finder, select **Void OSCQ.app**, **File**, **Get Info**.
   Expected: Version equal to the tag, without the `v`.
8. In Terminal, read what the bundle and Spotlight hold:
   ```
   defaults read "/Applications/Void OSCQ.app/Contents/Info" CFBundleShortVersionString
   mdls -name kMDItemVersion "/Applications/Void OSCQ.app"
   ```
   Expected: both give the tag's version. If the first does and the second gives `(null)`, the bundle carries the version and the Finder's Version column waits for Spotlight to index the app.

Proves: the tagged CI run gives a macOS app that opens on this Mac, unsigned, through the documented procedure, with its icon and its version. Cannot prove: the first launch on other macOS versions, or on Intel if this Mac is Apple Silicon; the Windows and Linux artifacts, which need their own systems (section 4).

## 1. Vidvox OSCQuery Helper, on the local network

Setup: OSCQuery Helper running with its `SampleDocument`, its OSC target port set (1240 in the captured setup), and an OSC monitor listening on that port to see what arrives.

1. Open Void OSCQ. If macOS asks whether it may find devices on the local network, click **Allow**.
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
   Expected: every message of steps 4 to 6, direction `out`, with path, type tags and arguments as the monitor received them. **in** shows none of them, **out** all of them. **Export** writes a text file, its path shown in the status bar.

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
7. Move the fader in Max, and do not touch Void OSCQ.
   Expected: the value shown does not change by itself, since Max sends no notification; F5 shows the new value.
8. Quit Void OSCQ and open it again.
   Expected: the server reconnects with "OSC localhost:30338 UDP" and "port set by the user"; sends reach Max. **Infer OSC port** goes back to 30339, "port inferred from the HTTP port"; set 30338 again before going on.
9. Close the patch in Max, then click the server on the left to reconnect.
   Expected: the status bar says the connection failed once, with the system's reason (for example "Connection refused"). The tree stays; its read write nodes still show `rw`, have no widget and say "cannot send", the reason on hover. Never "read only".
10. Open the patch again and click the server.
   Expected: connected, the widgets back, no error left in the status bar.

Proves: a server that ignores the `HOST_INFO` query is used with the proposal's defaults and no error, reading values from a real server, sending to a real server on the port set by the user where the inferred one is wrong, that port kept across a restart, the honest display of a WebSocket that accepts and stays silent. Cannot prove: live values (Max does not notify), discovery (Max does not advertise and listens on 127.0.0.1 only).

## 3. Synthetic server, on the same Mac

Setup, from a clone of the repository at the release tag:

```
cargo run -p oscq-core --example synthetic_server --release -- --port 5678 --stream 2
```

1. In Void OSCQ, **+ manual**, `127.0.0.1:5678`, connect.
   Expected: 5050 nodes in the status bar, the tree shown at once and smooth to scroll.
2. Expand `/g0`.
   Expected: within a second, "live" in the status bar; the values of the visible rows change twice a second by themselves, each row its own value. The server's terminal shows the number of listens, close to the number of visible rows.
3. Scroll `/g0` down, then collapse it.
   Expected: the server's listen count follows the rows on screen, and drops to 0 when collapsed.
4. Stop the server with Ctrl C.
   Expected: the status bar shows "connected, socket lost" and the retry delay, growing on each attempt, up to 15 s.
5. Start the server again with the same command.
   Expected: back to "live" within the retry delay; with `/g0` expanded, values move again, without touching the app.
6. With the pointer outside the window and nothing changing, watch the CPU of Void OSCQ in Activity Monitor for ten seconds.
   Expected: about 0 %.

Proves: the nominal WebSocket path end to end, listen and ignore following the rows on screen, reconnection with backoff and listens sent again, idle with no repaint. Cannot prove: interoperability with a third party server: the synthetic server is this project's own.

## 4. Windows and Linux, when available

Not required for the macOS release. On each system available:

1. Download its artifact, follow the README's first launch procedure for that system.
   Expected: the window opens; on Windows, no console window. The executable in Explorer, the window and the taskbar show the application's icon; on Linux, the icon in the application menu once installed as the README says.
2. Run section 3 with the synthetic server on that machine, or section 1 if a Helper runs on the LAN.
   Expected: the same results.

Proves: the artifacts of that system. Cannot prove: the systems not run.

## 5. Interface: server list, pinned nodes, presets, traffic

Setup: the synthetic server of section 3 running, Max 9 with its patch as in section 2, both entered with **+ manual**.

1. Look at the window with the synthetic server selected and `/g0` expanded.
   Expected: in the tree, three columns: names, widgets on one vertical line, type tags and flags right aligned; a thin vertical guide per level of indentation. Paths, values and type tags in a monospace face, the rest in a proportional one. The status bar is one line: state, server, OSC destination, node count, then the last message. Groups apart by space, no rules between them.
2. Switch **Theme** to light, then dark, then system; quit and open again.
   Expected: each one applied at once; system follows the setting of macOS; the last choice kept after the restart.
3. In the server pane, look at the **Discovered** and **Manual** sections.
   Expected: the manual entries under **Manual**, each with an **x**; a discovered server has none.
4. Right click `/g0/p0`, **Pin**; the same for `/g0/p1` and `/g0/p2`. Collapse `/g0`, open the **JSON** tab.
   Expected: the three nodes in **Pinned** on the right, with their widgets, whatever the tree or the tab shows. Moving a pinned widget sends, as in the tree.
5. Set the three pinned values to 0.1, 0.2, 0.3. In **Presets**, click **set** on slot 01.
   Expected: a note "Slot 01: 3 values of pinned methods captured". The number 01 becomes clickable.
6. Move the three values elsewhere, then click **01**.
   Expected: the three widgets back to 0.1, 0.2, 0.3 at once, with no confirmation. The **Traffic** tab shows one line, its time then `out preset 01  (3 messages)`; clicking it lists the three messages.
7. In **Traffic**, click **in**, then **out**, then **all**.
   Expected: **in** shows only values received, **out** only what was sent, **all** both.
8. Right click a node, **Copy path**, and paste it somewhere.
   Expected: its full path.
9. Quit and open again, select the synthetic server.
   Expected: the three pinned nodes and slot 01 still there.
10. Select Max 9, click **01**.
   Expected: nothing sent, the note says "Slot 01 recalled: 0 sent, 3 not found on this server (captured on ...)". Nothing blocks.
11. Click the **x** of Max 9's manual entry, then **+ manual** `localhost:30339` again.
   Expected: the entry leaves the list at once; added again, its OSC port is inferred from the HTTP port, 30339: the port set earlier was forgotten with the entry.

Proves: the interface rework seen by a person, the server list removal, the pinned panel across tabs and restarts, a capture and recall cycle on the same server and on another one, the traffic direction filter. Cannot prove: anything beyond this Mac and these two servers.

## Known limit: WebSocket interoperability with a third party server

Not proven by this checklist: that Void OSCQ receives the notifications of a third party OSCQuery server over its WebSocket, values changed inside the server appearing without user action, and `PATH_*` commands reloading the tree. The only server on which the notification path is proven is this project's synthetic server (section 3), written from this project's own reading of the proposal: it shows the client agrees with itself, not with anyone else.

The two third party servers tested do not notify:

| Server | WebSocket, as observed | What it does not do |
| --- | --- | --- |
| Vidvox OSCQuery Helper | Its `HOST_INFO` (`tests/fixtures/helper-hostinfo.json`) gives no `WS_IP` or `WS_PORT` and lists `PATH_CHANGED` but not `LISTEN`; `LISTEN` is commented out in its source (docs/protocol-notes.md section 4). Every node is write only, `ACCESS` 2, and `?VALUE` answers `{}`. Whether it accepts a socket on its HTTP port was not observed | No `LISTEN`, so no value frames; no value of its own to push |
| Max 9 | Accepts a WebSocket on its HTTP port 30339 and accepts `LISTEN` (`/test/param/fader/raw`, checked with `websocat`, docs/protocol-notes.md section 12) | Sends nothing on the socket: no binary value frame when a parameter moves, no `PATH_*` command |

Untested against a real server, in consequence: binary OSC value frames as a third party encodes them (bundles, type tags, timetags); `PATH_ADDED`, `PATH_REMOVED`, `PATH_RENAMED` and `PATH_CHANGED` as a third party sends them; `LISTEN` and `IGNORE` honoured by a server; the reconnection after a third party server drops its socket.

To lift it, one server that notifies over its WebSocket and is not this project's, for example VDMX or an ossia based patch, both named in PRD 12; that they notify is what their makers state, not something checked here. Then:

1. Connect to it and expand a container: the status bar says "live" only once the server has sent a frame.
2. Change a value inside the server, without touching Void OSCQ: the row updates, read, not marked "sent"; the Traffic tab logs it as `in`.
3. Add or remove a node inside the server: the tree follows without a refresh.
4. Capture the frames with `websocat` while doing 2 and 3, and commit them under `tests/fixtures` with their provenance, so that the client's decoding of that server is tested in CI as the Helper and Max documents are.

Until then the limit stands, and no release note may say that live values work with third party servers.

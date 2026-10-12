# Release acceptance

The manual checklist run by the human before a release, on the artifacts of the release tag, not on a build from source. Each step gives what to do and what must be seen. A step whose result differs is a failed release: note what was seen instead.

Three servers are available and covered: Vidvox OSCQuery Helper, Max 9, and this project's synthetic server. VDMX and an ossia based patch, named in PRD 12, are not available and are not covered. No covered server is a third party server that notifies over its WebSocket: the live update of a value changed inside a real server stays unproven, see the end.

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
5. Click **About**, at the bottom of the server list.
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
   Expected: the tree loads, 22 nodes in the status bar. The status bar shows the OSC destination with the Helper's target port and "port advertised", and "values on refresh only".
3. Expand `/test`.
   Expected: every method has a widget and no value readout, flags `w`: the Helper declares every node write only. `/test/my_true` and `/test/my_false` show a toggle.
4. Drag the slider of `/test/my_float` and release it.
   Expected: the monitor receives `/test/my_float` with one `f` argument following the slider, at most one message per 8 ms while dragging, and the final value on release. The row then shows the value sent, in italics, marked "(sent)", and the slider stays where it was released.
5. Set `/test/my_longlong` to 42.
   Expected: the monitor receives `/test/my_longlong ,h 42`.
6. Click the toggle of `/test/my_true`; click the button of `/test/my_infinity`.
   Expected: the monitor receives a `T` or `F` message, then an `I` message.
7. Select `/test`, open the **JSON** tab, click **Copy** and paste the text somewhere.
   Expected: the document of `/test` exactly as the Helper sent it, `\/` escapes included.
8. Open the **Traffic** tab.
   Expected: every message of steps 4 to 6, direction `out`, with path, type tags and arguments as the monitor received them. **Export** writes a text file, its path shown in the status line.

Proves: discovery by Bonjour on the LAN, `HOST_INFO` with an advertised port, sending OSC to a real server with the declared types, the write only display, the JSON and traffic tabs. Cannot prove: values read from the server (the Helper serves none), the WebSocket (the Helper has none).

## 2. Max 9, on the same Mac

Setup: Max 9 running `tests/fixtures/max/test.maxpat`, which serves OSCQuery on `localhost:30339`. Close OSCQuery Helper.

1. Click **+ manual**, enter `localhost:30339`, connect.
   Expected: the tree loads with `/test/param/gain`, `/test/param/freq` and `/test/param/fader`, each holding ten nodes (`longname`, `min`, `max`, `raw`, `normalized`...), with their values. The status bar shows "port inferred from the HTTP port" and "HOST_INFO ignored by the server, defaults used", and no error.
2. Wait a few seconds after the tree loads.
   Expected: "WebSocket connected, no update received yet: values on refresh only". Never "values live".
3. Change `/test/param/fader/normalized`, a read write node, with its widget.
   Expected: the fader moves in Max. The row shows the value sent until a value is read.
4. Select `/test/param/fader/normalized` and press F5.
   Expected: the value read from Max, in plain text, no longer marked "(sent)".
5. Move the fader in Max, and do not touch Void OSCQ.
   Expected: the value shown does not change by itself, since Max sends no notification; F5 shows the new value.
6. Close the patch in Max, then click the server on the left to reconnect.
   Expected: the status bar says the connection failed once, with the system's reason (for example "Connection refused"). The tree stays; its read write nodes still show `rw`, have no widget and say "cannot send", the reason on hover. Never "read only".
7. Open the patch again and click the server.
   Expected: connected, the widgets back, no error left in the status bar.

Proves: a server that ignores the `HOST_INFO` query is used with the proposal's defaults and no error, reading values from a real server, sending to a real server on the inferred port, the honest display of a WebSocket that accepts and stays silent. Cannot prove: live values (Max does not notify), discovery (Max does not advertise and listens on 127.0.0.1 only).

## 3. Synthetic server, on the same Mac

Setup, from a clone of the repository at the release tag:

```
cargo run -p oscq-core --example synthetic_server --release -- --port 5678 --stream 2
```

1. In Void OSCQ, **+ manual**, `127.0.0.1:5678`, connect.
   Expected: 5050 nodes in the status bar, the tree shown at once and smooth to scroll.
2. Expand `/g0`.
   Expected: within a second, "values live over the WebSocket"; the values of the visible rows change twice a second by themselves, each row its own value. The server's terminal shows the number of listens, close to the number of visible rows.
3. Scroll `/g0` down, then collapse it.
   Expected: the server's listen count follows the rows on screen, and drops to 0 when collapsed.
4. Stop the server with Ctrl C.
   Expected: the status bar shows the WebSocket lost and the retry delay, growing on each attempt, up to 15 s.
5. Start the server again with the same command.
   Expected: back to "values live" within the retry delay; with `/g0` expanded, values move again, without touching the app.
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

## Open item

A value changed inside a real third party server that notifies over its WebSocket, VDMX or ossia, updating in Void OSCQ without user action. Neither OSCQuery Helper nor Max 9 sends notifications; the synthetic server is the only proof so far. Stays open until such a server is available.

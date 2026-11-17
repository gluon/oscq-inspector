# Screenshots of the visual refresh

Forty eight windows: every view, before and after the refresh of 2026-11-17, in both themes and
at both widths. `before` is the build of `d3df218`, the commit this branch starts from; `after`
is this branch. The name of a file says which: `STATE-THEME-WIDTH-VIEW.png`.

| Part | Values |
| --- | --- |
| STATE | `before`, `after` |
| THEME | `dark`, `light` |
| WIDTH | `1280` for 1280 by 800, `640` for the 640 point minimum |
| VIEW | `tree`, `json`, `traffic`, `map`, `about`, `nofound` |

## How they were taken

Against the PRISM demo device of prism-oscquery 0.5.0, found over Bonjour, and two synthetic
servers of this repository entered by hand, `--peers serve` so that the map has edges to draw.
No LiveOSCQuery server, and no server of any other machine: the network was taken down for the
session, so nothing but those three was reachable.

Each view is one launch, not a click: `OSCQ_INSPECTOR_STARTUP` opens a tab or the about box
(README, Tools). The window size, the theme and the pinned paths come from a config file in a
scratch `HOME`, so the person's own config was never touched. Anyone can take them again the
same way.

The captures are 1x: 1280 by 828 points and 640 by 828, the window with its title bar. They come
from a window capture of the application alone, which is the only capture on this machine that
does not take the whole screen with it. For an image at the display's full resolution, take it by
hand with the system's own screenshot once the window is open.

## What the `nofound` views show, and a limit of three of them

`nofound` is the server pane with nothing found, which PRD 9.1 asks to say since when it has been
searching, as a time of day.

Five of the eight show that message: the four `after` views and `before-dark-1280`. The other
three, `before-light-1280`, `before-dark-640` and `before-light-640`, show the state of its first
two seconds, "Searching the network". The reason is not the application and not the build: a
window no other window covers repaints on the timer the pane asks for, but these were captured
while the window sat behind another, where macOS composites nothing and the frame never changes.
`before-dark-1280` has the message and the other three do not, from the same build, which is what
says the difference is the capture and not the code. Both states are the pane with none found.

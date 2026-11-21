# The images of the README

Four windows of 0.5.1, taken on 2026-11-21: `tree-dark.png`, `tree-light.png`, `json-dark.png` and `traffic-dark.png`. The map of the README is older, [docs/images/map.png](../map.png), taken on 2026-11-18 against the PRISM demo device and two synthetic servers: its edges carry the packets a real server counted, which the synthetic server does not count.

## What they show

Two servers found over Bonjour, both the synthetic server of this repository:

- **AV rig**: `--doc crates/oscq-core/examples/demo-space.json`, the address space of a small audiovisual rig, 25 methods of every type the window has a widget for, with `--echo`, so that every value sent comes back over the WebSocket as from a server confirming it.
- **synthetic 5000**: the 5000 methods of the test suite, there so that the server list and the map hold more than one server.

No application other than these two, and no other machine: what the images show is what this repository serves. The values move because the server is run with `--stream`: they are the synthetic server's sine waves, not a real rig.

## How they were taken

On Linux, under Xvfb with Mesa's software renderer, by the window itself with the `screenshot` feature ([DEVELOPMENT.md](../../../DEVELOPMENT.md#tools)). `WINIT_X11_SCALE_FACTOR=2` draws a 1280 by 800 point window at 2560 by 1600 pixels. The fonts are compiled into the binary, so the window draws the same on the three systems; what differs on macOS is the title bar, which the window's own capture does not include, and the line under the preset grid, which names the option key there and alt here.

The servers, and a Bonjour advertisement of both on `127.0.0.1` with the Python `zeroconf` package:

```
cargo build --release -p oscq-inspector --features screenshot
cargo build --release -p oscq-core --example synthetic_server
target/release/examples/synthetic_server --port 5680 --peers serve --echo --doc crates/oscq-core/examples/demo-space.json --name "AV rig" --stream 4 &
target/release/examples/synthetic_server --port 5681 --peers serve --name "synthetic 5000" --stream 2 &
python3 advertise.py &
```

`advertise.py`:

```python
import time
from zeroconf import IPVersion, ServiceInfo, Zeroconf

zc = Zeroconf(ip_version=IPVersion.V4Only)
for name, port in [("AV rig", 5680), ("synthetic 5000", 5681)]:
    zc.register_service(ServiceInfo(
        "_oscjson._tcp.local.", f"{name}._oscjson._tcp.local.", port=port,
        parsed_addresses=["127.0.0.1"], server=f"{name.replace(' ', '-')}.local."))
time.sleep(3600)
```

Each image is one launch, with its own scratch `HOME` holding a `.config/void-oscq/config.json` of `{"window":{"x":0,"y":0,"width":1280,"height":800},"theme":"Dark","presets_open":true}` (`"Light"` for the light one). The tree:

```
HOME=/tmp/shot WINIT_X11_SCALE_FACTOR=2 \
OSCQ_INSPECTOR_SHOT=tree-dark.png OSCQ_INSPECTOR_SHOT_AFTER_MS=4000 \
OSCQ_INSPECTOR_SHOT_DO=connect-all,select:av-rig.local:5680,expand:/lights,expand:/lights/wash,expand:/synth,expand:/synth/amp,expand:/synth/filter,expand:/synth/osc1,expand:/video,expand:/video/layer1,pin:/synth/filter/cutoff,pin:/synth/amp/gain,pin:/lights/wash/color,pin:/lights/wash/intensity,pin:/meters/rms,pin:/meters/peak,capture:1,capture:4,capture:7,recall:1 \
xvfb-run -a -s "-screen 0 2800x1800x24" target/release/oscq-inspector
```

A discovered server is selected by its host name, lower cased and without the trailing dot, and its port: `av-rig.local:5680`. `json-dark.png` is the same words followed by `json`.

`traffic-dark.png` starts from the config the tree's launch left, its pins and its three slots, with AV rig run without `--stream`, so that the log holds the recalls alone: `connect-all,select:av-rig.local:5680,traffic,recall:4,recall:7,recall:1`. In one launch out of the three taken that way, the values coming back from the second recall were listed above the row of the recall that sent them, at the same millisecond. The image is one of the two others. Why the order changed is not established.

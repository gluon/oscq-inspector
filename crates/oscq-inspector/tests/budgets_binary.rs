//! PRD 10 budgets measured on the real binary, window and all, against the synthetic server's
//! 5000 node document: cold start, idle CPU with 50 active listens, idle repaints, resident
//! memory. Then the three map budgets, against eight synthetic servers that publish their
//! peers: frames at rest with the map tab hidden and shown, and the frame time on the map.
//! Idle is measured with no input reaching the window: keep the pointer outside it.
//! Needs a display, Linux or macOS, and a release build:
//!
//! ```text
//! cargo test --release -p oscq-inspector --test budgets_binary -- --ignored --nocapture --test-threads=1
//! ```

#![allow(clippy::unwrap_used)] // test code, unwrap is allowed outside production paths (PRD 11)

#[path = "../../oscq-core/tests/common/mod.rs"]
mod synthetic;

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use synthetic::{
    OscReceiver, PeersMode, Reply, SyntheticServer, borrowed, host_info_json, large_routes,
};

const LISTENS: usize = 50;
const IDLE_WINDOW: Duration = Duration::from_secs(10);
/// How far from its beat a frame may land before the map tests call it something other than
/// the timer's: half an interval. It is not a tolerance on the interval, which they do not
/// assert: it separates one beat from the next. Two repaints for one beat land milliseconds
/// apart, a beat missed is a whole interval, and the furthest a frame has strayed from its
/// beat on the machine that strays most, the macOS CI one, is 5.6 percent of an interval
/// (docs/decisions.md, 2026-11-13, "M7, the map poll tolerance").
const BEAT_SLACK: f64 = 0.5;

/// The config file location the binary computes, under a fresh base directory, and the
/// environment variable that points the binary at that base.
fn config_location(base: &Path) -> (PathBuf, &'static str) {
    if cfg!(target_os = "macos") {
        (
            base.join("Library/Application Support/void-oscq/config.json"),
            "HOME",
        )
    } else if cfg!(target_os = "windows") {
        (base.join("void-oscq/config.json"), "APPDATA")
    } else {
        (base.join("void-oscq/config.json"), "XDG_CONFIG_HOME")
    }
}

/// CPU time used so far by a process.
fn cpu_time(pid: u32) -> Duration {
    if cfg!(target_os = "linux") {
        // Fields 14 and 15 of /proc/PID/stat, user and system time in clock ticks of 1/100 s.
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).unwrap();
        let after_name = &stat[stat.rfind(')').unwrap() + 2..];
        let fields: Vec<&str> = after_name.split(' ').collect();
        let ticks: u64 = fields[11].parse::<u64>().unwrap() + fields[12].parse::<u64>().unwrap();
        Duration::from_millis(ticks * 10)
    } else {
        // `ps` prints [[dd-]hh:]mm:ss.cc on macOS.
        let out = ps(pid, "time=");
        let mut secs = 0.0;
        for part in out.trim().split(':') {
            secs = secs * 60.0 + part.parse::<f64>().unwrap();
        }
        Duration::from_secs_f64(secs)
    }
}

/// Resident memory in bytes.
fn rss(pid: u32) -> u64 {
    let kb: u64 = if cfg!(target_os = "linux") {
        let status = std::fs::read_to_string(format!("/proc/{pid}/status")).unwrap();
        status
            .lines()
            .find_map(|l| l.strip_prefix("VmRSS:"))
            .unwrap()
            .trim()
            .trim_end_matches("kB")
            .trim()
            .parse()
            .unwrap()
    } else {
        ps(pid, "rss=").trim().parse().unwrap()
    };
    kb * 1024
}

fn ps(pid: u32, field: &str) -> String {
    let out = Command::new("ps")
        .args(["-o", field, "-p", &pid.to_string()])
        .output()
        .unwrap();
    String::from_utf8(out.stdout).unwrap()
}

/// Lines the binary logs, with the time each arrived.
fn capture(child: &mut Child) -> Arc<Mutex<Vec<(Instant, String)>>> {
    let lines = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&lines);
    let stdout = child.stdout.take().unwrap();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            sink.lock().unwrap().push((Instant::now(), line));
        }
    });
    lines
}

fn wait_until(what: &str, limit: Duration, done: impl Fn() -> bool) {
    let end = Instant::now() + limit;
    while !done() {
        assert!(Instant::now() < end, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

struct Kill(Child);

impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// The binary running against synthetic servers, connected by its config, its log lines
/// captured. Killed and cleaned up on drop.
struct Launch {
    server: SyntheticServer,
    /// The servers of the map budgets, besides `server`.
    others: Vec<SyntheticServer>,
    child: Kill,
    lines: Arc<Mutex<Vec<(Instant, String)>>>,
    spawned: Instant,
    base: PathBuf,
    _rt: tokio::runtime::Runtime,
}

/// The servers of the map budgets (PRD 10): eight, each publishing its peers, three clients of
/// its own besides this application, 24 peers in all, each listening to a few methods.
const MAP_SERVERS: usize = 8;
const MAP_PEERS_EACH: usize = 3;

impl Launch {
    fn start() -> Self {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        let osc = rt.block_on(OscReceiver::bind("127.0.0.1:0")).unwrap();
        let routes = large_routes(5000, osc.port());
        let server = rt
            .block_on(SyntheticServer::start_ws("127.0.0.1:0", borrowed(&routes)))
            .unwrap();
        let key = format!("127.0.0.1:{}", server.addr.port());
        let pinned: Vec<String> = (0..LISTENS).map(|i| format!("/g0/p{i}")).collect();
        let config = serde_json::json!({
            "window": {"x": 0.0, "y": 0.0, "width": 1100.0, "height": 700.0},
            "manual_entries": [key],
            "last_selected": key,
            "pinned_paths": {key.clone(): pinned},
        });
        Self::spawn(rt, server, Vec::new(), config, "")
    }

    /// The eight servers of the map budgets, connected at launch, the window opened on the map
    /// tab or on the tree, the peers read every `poll_ms`.
    fn start_map(map: bool, poll_ms: u64) -> Self {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        let osc = rt.block_on(OscReceiver::bind("127.0.0.1:0")).unwrap();
        let mut servers: Vec<SyntheticServer> = (0..MAP_SERVERS)
            .map(|i| {
                let mut routes = large_routes(200, osc.port());
                routes.push((
                    "/?HOST_INFO".to_owned(),
                    Reply::json(host_info_json(&format!("map {i}"), osc.port(), true)),
                ));
                let s = rt
                    .block_on(SyntheticServer::start_ws("127.0.0.1:0", borrowed(&routes)))
                    .unwrap();
                s.ws.set_peers(PeersMode::Serve);
                let clients = (0..MAP_PEERS_EACH)
                    .map(|c| {
                        serde_json::json!({
                            "IP": format!("10.0.{i}.{}", c + 1),
                            "PORT": 40000 + c,
                            "NAME": format!("peer {i}.{c}"),
                            "LISTEN": [format!("/g0/p{c}"), format!("/g1/p{c}")],
                            "PACKETS": 10 * c,
                        })
                    })
                    .collect();
                s.ws.set_other_peers(clients, Vec::new());
                s
            })
            .collect();
        let keys: Vec<String> = servers
            .iter()
            .map(|s| format!("127.0.0.1:{}", s.addr.port()))
            .collect();
        let config = serde_json::json!({
            "window": {"x": 0.0, "y": 0.0, "width": 1100.0, "height": 700.0},
            "manual_entries": keys,
            "last_selected": keys[0],
            "peers_poll_ms": poll_ms,
        });
        let first = servers.remove(0);
        let startup = if map {
            "map,connect-all"
        } else {
            "connect-all"
        };
        Self::spawn(rt, first, servers, config, startup)
    }

    fn spawn(
        rt: tokio::runtime::Runtime,
        server: SyntheticServer,
        others: Vec<SyntheticServer>,
        config: serde_json::Value,
        startup: &str,
    ) -> Self {
        let base = std::env::temp_dir().join(format!(
            "oscq-inspector-budget-{}-{}",
            std::process::id(),
            server.addr.port()
        ));
        let _ = std::fs::remove_dir_all(&base);
        let (config_path, env_var) = config_location(&base);
        std::fs::create_dir_all(config_path.parent().unwrap()).unwrap();
        std::fs::write(&config_path, config.to_string()).unwrap();

        let spawned = Instant::now();
        let mut child = Command::new(env!("CARGO_BIN_EXE_oscq-inspector"))
            .env(env_var, &base)
            .env(
                "RUST_LOG",
                "oscq_inspector::startup=info,oscq_inspector::frame=trace,\
                 oscq_inspector::event=trace,oscq_inspector::frame_time=trace",
            )
            .env("OSCQ_INSPECTOR_STARTUP", startup)
            .env("NO_COLOR", "1")
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let lines = capture(&mut child);
        Launch {
            server,
            others,
            child: Kill(child),
            lines,
            spawned,
            base,
            _rt: rt,
        }
    }

    fn all_servers(&self) -> impl Iterator<Item = &SyntheticServer> {
        std::iter::once(&self.server).chain(&self.others)
    }

    /// The eight servers connected, each with this application's WebSocket open and named, and,
    /// on the map, each asked for its peers; then settled.
    fn settle_map(&self, map: bool) {
        wait_until("eight sockets named", Duration::from_secs(30), || {
            self.all_servers().all(|s| {
                s.ws.received()
                    .first()
                    .is_some_and(|t| t.contains("CLIENT_NAME"))
            })
        });
        if map {
            wait_until("eight servers asked", Duration::from_secs(30), || {
                self.all_servers()
                    .all(|s| s.ws.requests().iter().any(|t| t == "/?PEERS"))
            });
        }
        std::thread::sleep(Duration::from_secs(3));
    }

    /// `?PEERS` requests received by all the servers so far.
    fn peers_requests(&self) -> usize {
        self.all_servers()
            .map(|s| s.ws.requests().iter().filter(|t| *t == "/?PEERS").count())
            .sum()
    }

    /// Each frame of the idle window that logged it, with the time its line arrived: the CPU
    /// time eframe measured for the frame before it, its own pass and its rendering.
    fn frame_times_since(&self, since: Instant) -> Vec<(Instant, Duration)> {
        self.lines
            .lock()
            .unwrap()
            .iter()
            .filter(|(at, l)| *at >= since && l.contains("oscq_inspector::frame_time"))
            .filter_map(|(at, l)| {
                let ms: f64 = l.split("ms=").nth(1)?.split(' ').next()?.parse().ok()?;
                Some((*at, Duration::from_secs_f64(ms / 1000.0)))
            })
            .collect()
    }

    fn pid(&self) -> u32 {
        self.child.0.id()
    }

    /// From spawn to the first frame, and from `main` to the first frame as the binary logs it.
    fn first_frame(&self) -> (Duration, Duration) {
        let find = || {
            self.lines
                .lock()
                .unwrap()
                .iter()
                .find(|(_, l)| l.contains("first frame"))
                .cloned()
        };
        wait_until("the first frame", Duration::from_secs(20), || {
            find().is_some()
        });
        let (at, line) = find().unwrap();
        let from_main: f64 = line
            .split("first frame ")
            .nth(1)
            .and_then(|r| r.split(' ').next())
            .unwrap()
            .parse()
            .unwrap();
        (
            at - self.spawned,
            Duration::from_secs_f64(from_main / 1000.0),
        )
    }

    /// Connected, the 50 pinned methods listened to, then settled: layout and animations done.
    fn settle(&self) {
        wait_until("50 listens on the server", Duration::from_secs(20), || {
            self.server.ws.listens().len() == LISTENS
        });
        std::thread::sleep(Duration::from_secs(3));
    }

    fn lines_of(&self, target: &str) -> Vec<String> {
        self.lines
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, l)| l.contains(target))
            .map(|(_, l)| l.clone())
            .collect()
    }

    /// The frames run during an idle window, the network and discovery events applied meanwhile,
    /// and the CPU used, percent of one core.
    fn idle(&self) -> Idle {
        let pid = self.pid();
        let frames0 = self.frame_lines().len();
        let events0 = self.lines_of("oscq_inspector::event").len();
        let (cpu0, t0) = (cpu_time(pid), Instant::now());
        std::thread::sleep(IDLE_WINDOW);
        let (cpu1, t1) = (cpu_time(pid), Instant::now());
        Idle {
            frames: self.frame_lines().split_off(frames0),
            events: self.lines_of("oscq_inspector::event").split_off(events0),
            cpu: (cpu1 - cpu0).as_secs_f64() / (t1 - t0).as_secs_f64() * 100.0,
        }
    }

    /// The lines of the frame trace, with the time each arrived; not those of the frame time
    /// trace, whose target starts the same.
    fn frame_lines(&self) -> Vec<(Instant, String)> {
        self.lines
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, l)| l.contains("oscq_inspector::frame:"))
            .cloned()
            .collect()
    }
}

struct Idle {
    /// Each frame of the window: when its line arrived, and the line.
    frames: Vec<(Instant, String)>,
    events: Vec<String>,
    cpu: f64,
}

impl Idle {
    /// The seconds between consecutive frames.
    fn gaps(&self) -> Vec<f64> {
        self.frames
            .windows(2)
            .map(|w| (w[1].0 - w[0].0).as_secs_f64())
            .collect()
    }

    /// Seconds from the first frame of the window to its last.
    fn span(&self) -> f64 {
        match (self.frames.first(), self.frames.last()) {
            (Some(first), Some(last)) => (last.0 - first.0).as_secs_f64(),
            _ => 0.0,
        }
    }

    /// The input events that reached the window during the idle window, by kind, as each frame
    /// logged them: a pointer move, a key, a focus change.
    fn inputs(&self) -> std::collections::BTreeMap<&str, usize> {
        let mut inputs: std::collections::BTreeMap<&str, usize> = Default::default();
        for (_, f) in &self.frames {
            let list = f
                .split("inputs=[")
                .nth(1)
                .and_then(|r| r.split(']').next())
                .unwrap_or_default();
            for kind in list.split(", ").filter(|k| !k.is_empty()) {
                *inputs.entry(kind.trim_matches('"')).or_default() += 1;
            }
        }
        inputs
    }

    /// The network and discovery events of the window, by kind.
    fn event_kinds(&self) -> std::collections::BTreeMap<&str, usize> {
        let mut kinds: std::collections::BTreeMap<&str, usize> = Default::default();
        for e in &self.events {
            let kind = e
                .split("kind=")
                .nth(1)
                .unwrap_or("?")
                .trim()
                .trim_matches('"');
            *kinds.entry(kind).or_default() += 1;
        }
        kinds
    }

    /// An idle window that received input did not measure idle: each input event costs frames
    /// and CPU, three frames at least. Neither did one in which the window learnt something it
    /// had to show: a server found or gone stale on the local network wakes it as rightly as a
    /// value would, and only `expected` event kinds, those the test itself provokes, leave the
    /// window with nothing new. Both fail as an invalid measurement, never as a budget overrun.
    fn assert_valid(&self, expected: &[&str]) {
        let inputs = self.inputs();
        if !inputs.is_empty() {
            let kinds: Vec<String> = inputs.iter().map(|(k, n)| format!("{n} {k}")).collect();
            panic!(
                "invalid measurement, not a budget overrun: {} input events reached the window \
                 during the {} s idle window ({}). Keep the pointer outside the window, touch \
                 nothing, and run again.",
                inputs.values().sum::<usize>(),
                IDLE_WINDOW.as_secs(),
                kinds.join(", ")
            );
        }
        let unexpected: Vec<String> = self
            .event_kinds()
            .into_iter()
            .filter(|(kind, _)| !expected.contains(kind))
            .map(|(kind, n)| format!("{n} {kind}"))
            .collect();
        if !unexpected.is_empty() {
            panic!(
                "invalid measurement, not a budget overrun: the window was told something \
                 during the {} s idle window ({}), and drawing it is its job. Discovery \
                 events come from the local network: run this on a network where no OSCQuery \
                 or OSC service appears or expires, or in CI, which has none.",
                IDLE_WINDOW.as_secs(),
                unexpected.join(", ")
            );
        }
    }

    /// What woke the window, so that one failing run names the cause.
    fn print(&self) {
        println!(
            "IDLE real binary: {} frames and {} events in {} s, CPU {:.2} % of one core",
            self.frames.len(),
            self.events.len(),
            IDLE_WINDOW.as_secs(),
            self.cpu
        );
        for (n, cause) in causes(&self.frames) {
            println!("  {n} frames asked for by: {cause}");
        }
        for (kind, n) in self.inputs() {
            println!("  {n} input events {kind}");
        }
        for (kind, n) in self.event_kinds() {
            println!("  {n} events {kind}");
        }
    }
}

impl Drop for Launch {
    fn drop(&mut self) {
        let _ = self.child.0.kill();
        let _ = self.child.0.wait();
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

/// The 95th percentile of `v`, and its median and maximum.
fn percentiles(mut v: Vec<Duration>) -> (Duration, Duration, Duration) {
    v.sort();
    let at = |q: f64| v[((v.len() - 1) as f64 * q).round() as usize];
    (at(0.5), at(0.95), at(1.0))
}

/// What asked for each frame, most frequent first: the source locations egui recorded for the
/// repaint requests of the frame before, or none when the frame was woken by the operating
/// system or by input. `oscq-inspector/src/app.rs` at the notify callback means a network or
/// discovery event.
fn causes(frames: &[(Instant, String)]) -> Vec<(usize, String)> {
    let mut counts: std::collections::BTreeMap<String, usize> = Default::default();
    for (_, f) in frames {
        let cause = f
            .split("causes=")
            .nth(1)
            .and_then(|c| c.split(" inputs=").next())
            .map(|c| c.trim())
            .filter(|c| *c != "[]")
            .map(|c| {
                // Registry paths differ per machine; keep the crate and file.
                c.split(", ")
                    .map(|loc| {
                        loc.rsplit_once("/src/index.crates.io-")
                            .map_or(loc, |(_, r)| r.split_once('/').map_or(r, |(_, rest)| rest))
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_else(|| "no repaint request: woken by the OS or by input".to_owned());
        *counts.entry(cause).or_default() += 1;
    }
    let mut v: Vec<(usize, String)> = counts.into_iter().map(|(c, n)| (n, c)).collect();
    v.sort_by_key(|(n, _)| std::cmp::Reverse(*n));
    v
}

/// PRD 6.2: when idle the app must not repaint. The real window, connected, WebSocket open, 50
/// listens, nothing changing: not one frame in ten seconds. On failure, what asked for the frames.
#[test]
#[ignore = "real window: display needed"]
fn real_binary_does_not_repaint_when_idle() {
    let launch = Launch::start();
    launch.first_frame();
    launch.settle();
    let idle = launch.idle();
    idle.print();
    idle.assert_valid(&[]);
    assert!(idle.frames.is_empty(), "the window repaints when idle");
}

/// Budget 1, cold start, over five launches in a row: from spawn to the first frame, split into
/// the time before `main` (process creation, loading) and the time from `main` to the first frame
/// (the app). One launch first, not measured: macOS Gatekeeper checks a freshly built binary on
/// its first launch, once, which is not the app's start.
#[test]
#[ignore = "budget: real window, display and release build needed"]
fn real_binary_cold_start_five_launches() {
    if cfg!(debug_assertions) {
        panic!("budgets are measured on a release build: cargo test --release");
    }
    let warm_up = Launch::start();
    warm_up.first_frame();
    drop(warm_up);
    let mut totals = Vec::new();
    for i in 1..=5 {
        let launch = Launch::start();
        let (total, from_main) = launch.first_frame();
        let before_main = total.saturating_sub(from_main);
        let ms = |d: Duration| d.as_secs_f64() * 1000.0;
        println!(
            "BUDGET cold start, launch {i}: {:.1} ms from spawn = {:.1} ms before main + {:.1} ms from main, budget 300 ms",
            ms(total),
            ms(before_main),
            ms(from_main)
        );
        totals.push(total);
    }
    assert!(
        totals.iter().all(|t| *t < Duration::from_millis(300)),
        "cold start"
    );
}

#[test]
#[ignore = "budget: real window, display and release build needed"]
fn real_binary_budgets() {
    if cfg!(debug_assertions) {
        panic!("budgets are measured on a release build: cargo test --release");
    }
    if cfg!(target_os = "windows") {
        panic!("CPU and memory are read with /proc or ps: not measurable here on Windows");
    }
    // Cold start is measured by `real_binary_cold_start_five_launches` alone, after a warm up.
    let launch = Launch::start();
    launch.first_frame();
    launch.settle();
    let idle = launch.idle();
    let memory = rss(launch.pid());
    let listens = launch.server.ws.listens().len();
    drop(launch);

    println!(
        "BUDGET idle CPU, {listens} active listens: {:.2} % of one core over {} s, budget 1 %",
        idle.cpu,
        IDLE_WINDOW.as_secs()
    );
    println!(
        "BUDGET idle repaints: {} frames in {} s, budget 0",
        idle.frames.len(),
        IDLE_WINDOW.as_secs()
    );
    idle.print();
    println!(
        "BUDGET resident memory, 5000 nodes connected: {:.1} MB, budget 150 MB",
        memory as f64 / 1_048_576.0
    );
    assert_eq!(listens, LISTENS);
    idle.assert_valid(&[]);
    assert!(idle.cpu < 1.0, "idle CPU");
    assert!(idle.frames.is_empty(), "idle repaints");
    assert!(memory < 150 * 1_048_576, "resident memory");
}

/// An idle window holding only the events the test provokes itself is a measurement of the
/// window at rest. These two need no window: they hold what `Idle::assert_valid` refuses.
fn window_told(kinds: &[&str]) -> Idle {
    Idle {
        frames: Vec::new(),
        events: kinds
            .iter()
            .map(|k| format!("2026-11-13T00:00:00Z TRACE oscq_inspector::event: kind=\"{k}\""))
            .collect(),
        cpu: 0.0,
    }
}

#[test]
fn an_idle_window_holding_only_what_the_test_asked_for_is_a_measurement() {
    window_told(&[]).assert_valid(&[]);
    window_told(&["Peers", "Peers"]).assert_valid(&["Peers"]);
}

/// A server found on the local network is not a budget overrun: the window drew it because it
/// learnt it, which is its job. The measurement is refused, and names what it was told.
#[test]
#[should_panic(
    expected = "invalid measurement, not a budget overrun: the window was told \
                           something during the 10 s idle window (1 ServerChanged, \
                           2 ServerFound)"
)]
fn an_idle_window_told_of_a_server_is_not_a_measurement() {
    window_told(&["Peers", "ServerFound", "ServerChanged", "ServerFound"]).assert_valid(&["Peers"]);
}

/// PRD 10, map budget: with the map tab not visible, zero idle repaints, as above, with eight
/// servers that publish their peers. The tree is shown: its listeners are read once, when it
/// opens, and nothing reads them again at rest. No server is asked for its peers during the
/// idle window.
#[test]
#[ignore = "real window: display needed"]
fn real_binary_map_hidden_does_not_repaint_when_idle() {
    let launch = Launch::start_map(false, 2000);
    launch.first_frame();
    launch.settle_map(false);
    let asked = launch.peers_requests();
    let idle = launch.idle();
    let asked_meanwhile = launch.peers_requests() - asked;
    idle.print();
    println!(
        "BUDGET idle repaints, map tab not visible, {MAP_SERVERS} servers: {} frames in {} s, budget 0; {asked_meanwhile} PEERS requests",
        idle.frames.len(),
        IDLE_WINDOW.as_secs()
    );
    idle.assert_valid(&[]);
    assert!(idle.frames.is_empty(), "the window repaints when idle");
    assert_eq!(
        asked_meanwhile, 0,
        "a server was asked for its peers at rest"
    );
}

/// PRD 10, map budget: with the map tab visible, eight servers, one repaint per poll interval
/// and no more. That budget is a rate, and the rate is what this test holds: the number of
/// frames of the idle window, every one of them asked for by the application's timer, each
/// reading the eight servers, and the span they cover. The gaps between them are printed and
/// not asserted one by one: a single gap is the interval plus the difference of two
/// deliveries by the operating system, and strays 5.6 percent of an interval on the macOS CI
/// machine while the span of four holds within 0.75 percent (docs/decisions.md, 2026-11-13).
#[test]
#[ignore = "real window: display needed"]
fn real_binary_map_visible_repaints_once_per_poll_interval() {
    let poll = Duration::from_millis(2000);
    let launch = Launch::start_map(true, poll.as_millis() as u64);
    launch.first_frame();
    launch.settle_map(true);
    let asked = launch.peers_requests();
    let idle = launch.idle();
    let asked_meanwhile = launch.peers_requests() - asked;
    idle.print();
    let (gaps, span) = (idle.gaps(), idle.span());
    let interval = poll.as_secs_f64();
    let nominal = gaps.len() as f64 * interval;
    println!(
        "BUDGET idle repaints, map tab visible, {MAP_SERVERS} servers: {} frames in {} s, \
         budget one per {} ms poll interval; gaps {gaps:.3?} s, spanning {span:.3} s of a \
         nominal {nominal:.3} s; {asked_meanwhile} PEERS requests",
        idle.frames.len(),
        IDLE_WINDOW.as_secs(),
        poll.as_millis()
    );
    idle.assert_valid(&["Peers"]);
    // The timer keeps its own beat, so a window holds as many of its beats as the interval
    // divides into it, or one more when the window opens just before one.
    let beats = (IDLE_WINDOW.as_secs_f64() / interval).floor() as usize;
    assert!(
        (beats..=beats + 1).contains(&idle.frames.len()),
        "{} frames in {} s at one per {} ms: neither {beats} nor {}",
        idle.frames.len(),
        IDLE_WINDOW.as_secs(),
        poll.as_millis(),
        beats + 1
    );
    for (n, cause) in causes(&idle.frames) {
        assert!(
            cause.contains("src/app.rs"),
            "{n} frames asked for by {cause}, not by the timer"
        );
    }
    let slack = BEAT_SLACK * interval;
    for gap in &gaps {
        assert!(
            *gap > interval - slack,
            "two frames {gap:.3} s apart, nearer than half an interval: one beat drew twice"
        );
    }
    assert!(
        (span - nominal).abs() < slack,
        "{} frames spanning {span:.3} s, not the {nominal:.3} s of as many beats",
        idle.frames.len()
    );
    assert!(
        asked_meanwhile >= MAP_SERVERS * (idle.frames.len() - 1)
            && asked_meanwhile <= MAP_SERVERS * idle.frames.len(),
        "{asked_meanwhile} PEERS requests for {} frames",
        idle.frames.len()
    );
}

/// PRD 10, map budget: frame time on the map tab, eight servers and 24 peers, under 8 ms at
/// p95. The CPU time eframe measures for each frame, the window's pass and its rendering, of
/// the frames the map's timer asks for, read every 250 ms here to have samples enough. A
/// measurement, printed; the budget is asserted.
#[test]
#[ignore = "budget: real window, display and release build needed"]
fn real_binary_map_frame_time() {
    if cfg!(debug_assertions) {
        panic!("budgets are measured on a release build: cargo test --release");
    }
    let launch = Launch::start_map(true, 250);
    launch.first_frame();
    launch.settle_map(true);
    let start = Instant::now();
    let idle = launch.idle();
    idle.assert_valid(&["Peers"]);
    let times: Vec<Duration> = launch
        .frame_times_since(start)
        .into_iter()
        .map(|(_, d)| d)
        .collect();
    let n = times.len();
    assert!(n >= 20, "only {n} frames measured");
    let (median, p95, max) = percentiles(times);
    let ms = |d: Duration| d.as_secs_f64() * 1000.0;
    println!(
        "BUDGET frame time on the map tab, {MAP_SERVERS} servers and {} peers: n {n} median {:.3} ms, p95 {:.3} ms, max {:.3} ms, budget 8 ms",
        MAP_SERVERS * MAP_PEERS_EACH,
        ms(median),
        ms(p95),
        ms(max)
    );
    assert!(p95 < Duration::from_millis(8), "frame time on the map");
}

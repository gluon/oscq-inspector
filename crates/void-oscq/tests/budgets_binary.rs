//! PRD 10 budgets measured on the real binary, window and all, against the synthetic server's
//! 5000 node document: cold start, idle CPU with 50 active listens, idle repaints, resident
//! memory. Needs a display, Linux or macOS, and a release build:
//!
//! ```text
//! cargo test --release -p void-oscq --test budgets_binary -- --ignored --nocapture
//! ```

#![allow(clippy::unwrap_used)] // test code, unwrap is allowed outside production paths (PRD 11)

#[path = "../../oscq-core/tests/common/mod.rs"]
mod synthetic;

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use synthetic::{OscReceiver, SyntheticServer, borrowed, large_routes};

const LISTENS: usize = 50;
const IDLE_WINDOW: Duration = Duration::from_secs(10);

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

/// The binary running against the synthetic server's 5000 nodes, connected by its config, with
/// 50 pinned methods listened to, its log lines captured. Killed and cleaned up on drop.
struct Launch {
    server: SyntheticServer,
    child: Kill,
    lines: Arc<Mutex<Vec<(Instant, String)>>>,
    spawned: Instant,
    base: PathBuf,
    _rt: tokio::runtime::Runtime,
}

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

        let base = std::env::temp_dir().join(format!(
            "void-oscq-budget-{}-{}",
            std::process::id(),
            server.addr.port()
        ));
        let _ = std::fs::remove_dir_all(&base);
        let (config_path, env_var) = config_location(&base);
        std::fs::create_dir_all(config_path.parent().unwrap()).unwrap();
        let pinned: Vec<String> = (0..LISTENS).map(|i| format!("/g0/p{i}")).collect();
        let config = serde_json::json!({
            "window": {"x": 0.0, "y": 0.0, "width": 1100.0, "height": 700.0},
            "manual_entries": [key],
            "last_selected": key,
            "pinned_paths": {key.clone(): pinned},
        });
        std::fs::write(&config_path, config.to_string()).unwrap();

        let spawned = Instant::now();
        let mut child = Command::new(env!("CARGO_BIN_EXE_void-oscq"))
            .env(env_var, &base)
            .env("RUST_LOG", "void_oscq::startup=info,void_oscq::frame=trace")
            .env("NO_COLOR", "1")
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let lines = capture(&mut child);
        Launch {
            server,
            child: Kill(child),
            lines,
            spawned,
            base,
            _rt: rt,
        }
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

    fn frame_lines(&self) -> Vec<String> {
        self.lines
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, l)| l.contains("void_oscq::frame"))
            .map(|(_, l)| l.clone())
            .collect()
    }

    /// The frames run during an idle window, and the CPU used meanwhile, percent of one core.
    fn idle(&self) -> (Vec<String>, f64) {
        let pid = self.pid();
        let (cpu0, frames0, t0) = (cpu_time(pid), self.frame_lines().len(), Instant::now());
        std::thread::sleep(IDLE_WINDOW);
        let (cpu1, t1) = (cpu_time(pid), Instant::now());
        let frames = self.frame_lines().split_off(frames0);
        let cpu = (cpu1 - cpu0).as_secs_f64() / (t1 - t0).as_secs_f64() * 100.0;
        (frames, cpu)
    }
}

impl Drop for Launch {
    fn drop(&mut self) {
        let _ = self.child.0.kill();
        let _ = self.child.0.wait();
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

/// What asked for each frame, most frequent first: the source locations egui recorded for the
/// repaint requests of the frame before, or none when the frame was woken by the operating
/// system or by input.
fn causes(frames: &[String]) -> Vec<(usize, String)> {
    let mut counts: std::collections::BTreeMap<String, usize> = Default::default();
    for f in frames {
        let cause = f
            .split("causes=")
            .nth(1)
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

fn print_causes(frames: &[String]) {
    for (n, cause) in causes(frames) {
        println!("  {n} frames: {cause}");
    }
}

/// PRD 6.2: when idle the app must not repaint. The real window, connected, WebSocket open, 50
/// listens, nothing changing: not one frame in ten seconds. On failure, what asked for the frames.
#[test]
#[ignore = "real window: display needed"]
fn real_binary_does_not_repaint_when_idle() {
    let launch = Launch::start();
    launch.first_frame();
    launch.settle();
    let (frames, cpu) = launch.idle();
    println!(
        "IDLE real binary: {} frames in {} s, CPU {cpu:.2} % of one core",
        frames.len(),
        IDLE_WINDOW.as_secs()
    );
    print_causes(&frames);
    assert!(frames.is_empty(), "the window repaints when idle");
}

/// Budget 1, cold start, over five launches in a row: from spawn to the first frame, split into
/// the time before `main` (process creation, loading, any first launch check by the OS) and the
/// time from `main` to the first frame (the app).
#[test]
#[ignore = "budget: real window, display and release build needed"]
fn budget_cold_start_five_launches() {
    if cfg!(debug_assertions) {
        panic!("budgets are measured on a release build: cargo test --release");
    }
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
fn budget_real_binary_start_idle_cpu_repaints_memory() {
    if cfg!(debug_assertions) {
        panic!("budgets are measured on a release build: cargo test --release");
    }
    if cfg!(target_os = "windows") {
        panic!("CPU and memory are read with /proc or ps: not measurable here on Windows");
    }
    let launch = Launch::start();
    let (cold, from_main) = launch.first_frame();
    launch.settle();
    let (frames, idle_cpu) = launch.idle();
    let memory = rss(launch.pid());
    let listens = launch.server.ws.listens().len();
    drop(launch);

    println!(
        "BUDGET cold start to first frame: {:.1} ms from spawn ({:.1} ms from main), budget 300 ms",
        cold.as_secs_f64() * 1000.0,
        from_main.as_secs_f64() * 1000.0
    );
    println!(
        "BUDGET idle CPU, {listens} active listens: {idle_cpu:.2} % of one core over {} s, budget 1 %",
        IDLE_WINDOW.as_secs()
    );
    println!(
        "BUDGET idle repaints: {} frames in {} s, budget 0",
        frames.len(),
        IDLE_WINDOW.as_secs()
    );
    print_causes(&frames);
    println!(
        "BUDGET resident memory, 5000 nodes connected: {:.1} MB, budget 150 MB",
        memory as f64 / 1_048_576.0
    );
    assert_eq!(listens, LISTENS);
    assert!(cold < Duration::from_millis(300), "cold start");
    assert!(idle_cpu < 1.0, "idle CPU");
    assert!(frames.is_empty(), "idle repaints");
    assert!(memory < 150 * 1_048_576, "resident memory");
}

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

#[test]
#[ignore = "budget: real window, display and release build needed"]
fn budget_real_binary_start_idle_cpu_repaints_memory() {
    if cfg!(debug_assertions) {
        panic!("budgets are measured on a release build: cargo test --release");
    }
    if cfg!(target_os = "windows") {
        panic!("CPU and memory are read with /proc or ps: not measurable here on Windows");
    }
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

    let base = std::env::temp_dir().join(format!("void-oscq-budget-{}", std::process::id()));
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
    let child = Kill(child);
    let pid = child.0.id();

    let first_frame = || {
        lines
            .lock()
            .unwrap()
            .iter()
            .find(|(_, l)| l.contains("first frame"))
            .cloned()
    };
    wait_until("the first frame", Duration::from_secs(20), || {
        first_frame().is_some()
    });
    let (at, line) = first_frame().unwrap();
    let cold = at - spawned;
    let in_app: f64 = line
        .split("first frame ")
        .nth(1)
        .and_then(|r| r.split(' ').next())
        .unwrap()
        .parse()
        .unwrap();

    wait_until("50 listens on the server", Duration::from_secs(20), || {
        server.ws.listens().len() == LISTENS
    });
    // Let connection, layout and animations settle.
    std::thread::sleep(Duration::from_secs(3));
    let frames = || {
        lines
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, l)| l.contains("void_oscq::frame"))
            .count()
    };
    let (cpu0, frames0, t0) = (cpu_time(pid), frames(), Instant::now());
    std::thread::sleep(IDLE_WINDOW);
    let (cpu1, frames1, t1) = (cpu_time(pid), frames(), Instant::now());
    let idle_cpu = (cpu1 - cpu0).as_secs_f64() / (t1 - t0).as_secs_f64() * 100.0;
    let idle_frames = frames1 - frames0;
    let memory = rss(pid);
    let listens = server.ws.listens().len();
    drop(child);
    let _ = std::fs::remove_dir_all(&base);

    println!(
        "BUDGET cold start to first frame: {:.1} ms from spawn ({in_app:.1} ms from main), budget 300 ms",
        cold.as_secs_f64() * 1000.0
    );
    println!(
        "BUDGET idle CPU, {listens} active listens: {idle_cpu:.2} % of one core over {} s, budget 1 %",
        IDLE_WINDOW.as_secs()
    );
    println!(
        "BUDGET idle repaints: {idle_frames} frames in {} s, budget 0",
        IDLE_WINDOW.as_secs()
    );
    println!(
        "BUDGET resident memory, 5000 nodes connected: {:.1} MB, budget 150 MB",
        memory as f64 / 1_048_576.0
    );
    assert_eq!(listens, LISTENS);
    assert!(cold < Duration::from_millis(300), "cold start");
    assert!(idle_cpu < 1.0, "idle CPU");
    assert_eq!(idle_frames, 0, "idle repaints");
    assert!(memory < 150 * 1_048_576, "resident memory");
}

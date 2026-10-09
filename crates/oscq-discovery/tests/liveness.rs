//! A server that vanishes without a goodbye: the publisher runs in a child process that is
//! killed, so it cannot send one. The liveness check must mark it stale; without the check,
//! nothing does within the same window.

#![allow(clippy::unwrap_used)] // test code, unwrap is allowed outside production paths (PRD 11)

use std::process::{Child, Command};
use std::time::{Duration, Instant};

use mdns_sd::{ServiceDaemon, ServiceInfo};
use oscq_discovery::{Discovery, DiscoveryEvent, MdnsSdConfig, MdnsSdDiscovery, OSCQUERY_SERVICE};

const CHILD_ENV: &str = "OSCQ_LIVENESS_PUBLISH";

/// Child process mode: publish one service named by the environment variable, then wait to be
/// killed. Run by the tests below through the test binary itself; does nothing otherwise.
#[test]
#[ignore = "helper run as a child process by the liveness tests"]
fn publisher_child() {
    let Ok(spec) = std::env::var(CHILD_ENV) else {
        return;
    };
    let (name, port) = spec.rsplit_once(':').unwrap();
    let daemon = ServiceDaemon::new().unwrap();
    let host = format!("oscq-live-{}.local.", std::process::id());
    let info = ServiceInfo::new(
        OSCQUERY_SERVICE,
        name,
        &host,
        "",
        port.parse().unwrap(),
        None::<std::collections::HashMap<String, String>>,
    )
    .unwrap()
    .enable_addr_auto();
    daemon.register(info).unwrap();
    std::thread::sleep(Duration::from_secs(120));
}

fn spawn_publisher(name: &str, port: u16) -> Child {
    Command::new(std::env::current_exe().unwrap())
        .args(["publisher_child", "--exact", "--ignored", "--nocapture"])
        .env(CHILD_ENV, format!("{name}:{port}"))
        .spawn()
        .unwrap()
}

fn wait_for(
    rx: &flume::Receiver<DiscoveryEvent>,
    within: Duration,
    pred: impl Fn(&DiscoveryEvent) -> bool,
) -> Option<DiscoveryEvent> {
    let deadline = Instant::now() + within;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match rx.recv_timeout(left) {
            Ok(e) if pred(&e) => return Some(e),
            Ok(_) => {}
            Err(_) => return None,
        }
    }
}

/// Starts discovery, a publisher, waits until it is found, kills it, and reports whether a
/// Stale event arrived within the window.
fn vanish(liveness: Option<Duration>, label: &str) -> bool {
    let name = format!("Vanishing {label} {}", std::process::id());
    let port = 30000 + (std::process::id() % 10000) as u16 + if liveness.is_some() { 0 } else { 1 };
    let mut discovery = MdnsSdDiscovery::new(MdnsSdConfig {
        liveness_interval: liveness,
        verify_timeout: Duration::from_secs(1),
    });
    let rx = discovery.start().unwrap();
    let mut child = spawn_publisher(&name, port);
    let found = wait_for(
        &rx,
        Duration::from_secs(15),
        |e| matches!(e, DiscoveryEvent::Found(s) if s.name == name),
    );
    child.kill().unwrap();
    child.wait().unwrap();
    assert!(found.is_some(), "{label}: publisher never found");
    let stale = wait_for(
        &rx,
        Duration::from_secs(10),
        |e| matches!(e, DiscoveryEvent::Stale(s) if s.name == name),
    );
    discovery.stop();
    stale.is_some()
}

#[test]
fn liveness_check_marks_a_killed_server_stale() {
    assert!(vanish(Some(Duration::from_secs(2)), "checked"));
}

#[test]
fn without_liveness_check_a_killed_server_stays_listed() {
    assert!(!vanish(None, "unchecked"));
}

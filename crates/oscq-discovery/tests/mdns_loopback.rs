//! End to end through real mDNS on this machine: a service registered by one `mdns-sd` daemon is
//! found by `MdnsSdDiscovery` running another.

#![allow(clippy::unwrap_used)] // test code, unwrap is allowed outside production paths (PRD 11)

use std::collections::HashSet;
use std::time::{Duration, Instant};

use mdns_sd::{ServiceDaemon, ServiceInfo};
use oscq_core::ServiceKind;
use oscq_discovery::{Discovery, DiscoveryEvent, MdnsSdDiscovery, OSC_SERVICE, OSCQUERY_SERVICE};

/// Collects events until `until` returns true or the deadline passes.
fn collect(
    rx: &flume::Receiver<DiscoveryEvent>,
    within: Duration,
    mut until: impl FnMut(&[DiscoveryEvent]) -> bool,
) -> Vec<DiscoveryEvent> {
    let deadline = Instant::now() + within;
    let mut seen = Vec::new();
    while !until(&seen) {
        let left = deadline.saturating_duration_since(Instant::now());
        match rx.recv_timeout(left) {
            Ok(e) => seen.push(e),
            Err(_) => break,
        }
    }
    seen
}

#[test]
fn finds_registered_services_once_each_and_marks_goodbye_stale() {
    let tag = std::process::id();
    let query_name = format!("OSCQuery Helper e2e {tag}");
    let osc_name = format!("Plain OSC e2e {tag}");
    let host = format!("oscq-e2e-{tag}.local.");
    let (query_port, osc_port) = (40000 + (tag % 10000) as u16, 50000 + (tag % 10000) as u16);

    let publisher = ServiceDaemon::new().unwrap();
    let query_info = ServiceInfo::new(
        OSCQUERY_SERVICE,
        &query_name,
        &host,
        "",
        query_port,
        None::<std::collections::HashMap<String, String>>,
    )
    .unwrap()
    .enable_addr_auto();
    let query_fullname = query_info.get_fullname().to_owned();
    publisher.register(query_info).unwrap();
    let osc_info = ServiceInfo::new(
        OSC_SERVICE,
        &osc_name,
        &host,
        "",
        osc_port,
        None::<std::collections::HashMap<String, String>>,
    )
    .unwrap()
    .enable_addr_auto();
    publisher.register(osc_info).unwrap();

    let mut discovery = MdnsSdDiscovery::default();
    let started = Instant::now();
    let rx = discovery.start().unwrap();

    // An event about the machine rather than about a server carries no server: `LocalNetwork`.
    let is_ours = |e: &DiscoveryEvent| {
        e.server()
            .is_some_and(|s| s.name == query_name || s.name == osc_name)
    };
    let found = collect(&rx, Duration::from_secs(10), |seen| {
        seen.iter()
            .filter(|e| matches!(e, DiscoveryEvent::Found(_)) && is_ours(e))
            .count()
            >= 2
    });
    let first_seen = started.elapsed();
    // Let further announcements on other interfaces arrive, then count distinct servers.
    let more = collect(&rx, Duration::from_secs(2), |_| false);
    let ours: Vec<&DiscoveryEvent> = found.iter().chain(&more).filter(|e| is_ours(e)).collect();
    let query_ids: HashSet<_> = ours
        .iter()
        .filter(|e| e.server().is_some_and(|s| s.name == query_name))
        .filter_map(|e| e.server().map(|s| s.id))
        .collect();
    let osc_ids: HashSet<_> = ours
        .iter()
        .filter(|e| e.server().is_some_and(|s| s.name == osc_name))
        .filter_map(|e| e.server().map(|s| s.id))
        .collect();
    assert_eq!(
        query_ids.len(),
        1,
        "OSCQuery service found exactly once: {ours:?}"
    );
    assert_eq!(osc_ids.len(), 1, "OSC service found exactly once: {ours:?}");
    eprintln!("both services found after {first_seen:?}");

    let query = ours
        .iter()
        .rev()
        .find(|e| e.server().is_some_and(|s| s.name == query_name))
        .unwrap()
        .server()
        .unwrap();
    assert_eq!(query.kind, ServiceKind::OscQuery);
    assert_eq!(query.port, query_port);
    assert!(!query.addresses.is_empty());
    assert!(query.http_base().is_some());
    let osc = ours
        .iter()
        .rev()
        .find(|e| e.server().is_some_and(|s| s.name == osc_name))
        .unwrap()
        .server()
        .unwrap();
    assert_eq!(osc.kind, ServiceKind::OscOnly);
    assert_eq!(osc.http_base(), None);

    // Goodbye: unregistering sends records with TTL 0.
    publisher
        .unregister(&query_fullname)
        .unwrap()
        .recv_timeout(Duration::from_secs(5))
        .unwrap();
    let after = collect(&rx, Duration::from_secs(10), |seen| {
        seen.iter()
            .any(|e| matches!(e, DiscoveryEvent::Stale(s) if s.name == query_name))
    });
    let stale = after
        .iter()
        .find(|e| matches!(e, DiscoveryEvent::Stale(s) if s.name == query_name))
        .unwrap_or_else(|| panic!("no Stale event after goodbye: {after:?}"));
    assert!(stale.server().unwrap().stale);
    assert!(query_ids.contains(&stale.server().unwrap().id));

    discovery.stop();
    let _ = publisher.shutdown();
}

/// The defect of 2026-11-18, on the human's Mac: on the first launch of a new build, macOS had
/// not yet decided whether this application may reach the local network, every multicast send
/// was refused with "No route to host", and `mdns-sd` said nothing about it. The window showed
/// "none found", a claim about a network nothing had been asked of (CLAUDE.md law 1). Worse,
/// `mdns-sd` doubles the gap between its queries, measured here at 1, 3, 7, 15, 31, 63 and 127 s
/// and capped at an hour by its source, so once access arrived the next query could be minutes
/// away and only a relaunch helped.
///
/// What this test can hold, on a machine whose network works, is the state machine: the system's
/// answer is given by the test, refused and then allowed, and discovery must report which of the
/// two it is, must keep asking while it is refused rather than deciding once at the start, and
/// must report the opening within `access_interval` of it happening. At that moment it sends a
/// real query itself, which is what gets the answers in without waiting for `mdns-sd`'s own
/// schedule.
///
/// What it cannot hold here: that a query would otherwise never leave. This machine's network
/// works, so `mdns-sd` finds servers whatever the test's probe answers, and a refusal cannot be
/// imposed on it from inside the process. That half is the manual check of docs/acceptance.md.
#[test]
fn the_local_network_state_is_asked_again_until_it_opens() {
    /// Within this of the system letting queries out, discovery says so and has sent one.
    const RECOVERY: Duration = Duration::from_secs(5);
    const REFUSED_FOR: Duration = Duration::from_secs(4);
    const ASK_EVERY: Duration = Duration::from_millis(500);

    let opens = Instant::now() + REFUSED_FOR;
    let asked = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counter = asked.clone();
    let probe: oscq_discovery::AccessProbe = std::sync::Arc::new(move || {
        counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if Instant::now() < opens {
            // What macOS answers while it has not decided: EHOSTUNREACH, "No route to host".
            Err(std::io::Error::from_raw_os_error(65))
        } else {
            Ok(())
        }
    });

    let mut discovery = MdnsSdDiscovery::new(oscq_discovery::MdnsSdConfig {
        access_probe: Some(probe),
        access_interval: ASK_EVERY,
        ..Default::default()
    });
    let rx = discovery.start().unwrap();

    let refused = |e: &DiscoveryEvent| {
        matches!(
            e,
            DiscoveryEvent::LocalNetwork(oscq_discovery::LocalNetwork::Refused { .. })
        )
    };
    let open = |e: &DiscoveryEvent| {
        matches!(
            e,
            DiscoveryEvent::LocalNetwork(oscq_discovery::LocalNetwork::Open)
        )
    };

    // While the system refuses, discovery says so, and says it before claiming anything else.
    let first = collect(&rx, Duration::from_secs(2), |seen| seen.iter().any(refused));
    assert!(
        first.iter().any(refused),
        "the refusal is reported while the system refuses: {first:?}"
    );
    assert!(
        !first.iter().any(open),
        "a refusal is not reported as an opening: {first:?}"
    );

    // It keeps asking rather than deciding once: several times over the refusal.
    let deadline = opens + RECOVERY;
    let rest = collect(
        &rx,
        deadline.saturating_duration_since(Instant::now()),
        |seen| seen.iter().any(open),
    );
    let took = Instant::now();
    assert!(
        rest.iter().any(open),
        "the opening is reported within {RECOVERY:?} of it happening: {rest:?}"
    );
    assert!(
        took <= deadline,
        "the opening was reported later than {RECOVERY:?} after it happened"
    );
    let times = asked.load(std::sync::atomic::Ordering::Relaxed);
    assert!(
        times >= (REFUSED_FOR.as_secs_f64() / ASK_EVERY.as_secs_f64()) as usize,
        "the system is asked again while it refuses, not once at the start: asked {times} times"
    );
    discovery.stop();
}

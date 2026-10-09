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

    let is_ours = |e: &DiscoveryEvent| e.server().name == query_name || e.server().name == osc_name;
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
        .filter(|e| e.server().name == query_name)
        .map(|e| e.server().id)
        .collect();
    let osc_ids: HashSet<_> = ours
        .iter()
        .filter(|e| e.server().name == osc_name)
        .map(|e| e.server().id)
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
        .find(|e| e.server().name == query_name)
        .unwrap()
        .server();
    assert_eq!(query.kind, ServiceKind::OscQuery);
    assert_eq!(query.port, query_port);
    assert!(!query.addresses.is_empty());
    assert!(query.http_base().is_some());
    let osc = ours
        .iter()
        .rev()
        .find(|e| e.server().name == osc_name)
        .unwrap()
        .server();
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
    assert!(stale.server().stale);
    assert!(query_ids.contains(&stale.server().id));

    discovery.stop();
    let _ = publisher.shutdown();
}

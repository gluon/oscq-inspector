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
/// Then the regression of the same day, in the fix itself: the system takes the sends of a build
/// it has not been told about, after refusing them for the first seconds, and drops the packets.
/// A send that is taken was read as the network being open, so the window went back to saying
/// "none found", and the asking stopped there, which is also what left it blind until a
/// relaunch. Only an answer settles it, and an answer heard on loopback alone is not one: that
/// is the half this test holds on a real wire.
///
/// What it cannot hold here: that a query would otherwise never leave. This machine's network
/// works, so `mdns-sd` finds what is there whatever the test's probe answers, and neither a
/// refusal nor the dropping can be imposed on it from inside the process. That half is the
/// manual check of docs/acceptance.md.
#[test]
fn the_local_network_is_asked_again_until_something_answers() {
    const REFUSED_FOR: Duration = Duration::from_secs(4);
    const ASK_EVERY: Duration = Duration::from_millis(500);

    // A server of this machine advertised at a loopback address, as the system's own responder
    // advertises one on its loopback interface: found, and no answer about the local network.
    let tag = std::process::id();
    let name = format!("Loopback only e2e {tag}");
    let port = 41000 + (tag % 10000) as u16;
    let publisher = ServiceDaemon::new().unwrap();
    publisher
        .register(
            ServiceInfo::new(
                OSCQUERY_SERVICE,
                &name,
                &format!("oscq-lo-{tag}.local."),
                "127.0.0.1",
                port,
                None::<std::collections::HashMap<String, String>>,
            )
            .unwrap(),
        )
        .unwrap();

    let opens = Instant::now() + REFUSED_FOR;
    let asked = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counter = asked.clone();
    let probe: oscq_discovery::AccessProbe = std::sync::Arc::new(move || {
        counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if Instant::now() < opens {
            // What macOS answers while it has not decided: EHOSTUNREACH, "No route to host".
            Err(std::io::Error::from_raw_os_error(65))
        } else {
            // And what it answers afterwards, having decided nothing: it takes the packet.
            Ok(())
        }
    });

    let mut discovery = MdnsSdDiscovery::new(oscq_discovery::MdnsSdConfig {
        access_probe: Some(probe),
        access_interval: ASK_EVERY,
        retry_interval: ASK_EVERY,
        ..Default::default()
    });
    let rx = discovery.start().unwrap();

    let network = |e: &DiscoveryEvent| match e {
        DiscoveryEvent::LocalNetwork(n) => Some(n.clone()),
        _ => None,
    };
    let is = |e: &DiscoveryEvent, state: &oscq_discovery::LocalNetwork| {
        network(e).as_ref() == Some(state)
    };
    let heard = oscq_discovery::LocalNetwork::Heard;

    // While the system refuses, discovery says so, and says nothing else about the network.
    let first = collect(&rx, Duration::from_secs(2), |seen| {
        seen.iter().any(|e| {
            matches!(
                network(e),
                Some(oscq_discovery::LocalNetwork::Refused { .. })
            )
        })
    });
    assert!(
        first.iter().any(|e| matches!(
            network(e),
            Some(oscq_discovery::LocalNetwork::Refused { .. })
        )),
        "the refusal is reported while the system refuses: {first:?}"
    );
    assert!(
        !first.iter().any(|e| is(e, &heard)),
        "a refusal is not reported as an answer: {first:?}"
    );

    // Long enough for the refusal to end and for several queries to be taken after it.
    let rest = collect(&rx, REFUSED_FOR + Duration::from_secs(4), |_| false);
    let all: Vec<&DiscoveryEvent> = first.iter().chain(&rest).collect();
    let ours = all
        .iter()
        .find(|e| e.server().is_some_and(|s| s.name == name))
        .unwrap_or_else(|| panic!("the loopback server is found: {all:?}"))
        .server()
        .unwrap();
    // The premise of the whole design, read off a real wire: a server advertised at a loopback
    // address arrives with loopback addresses only, so hearing it is no answer about the local
    // network. It is how the window could show one server of this machine while being deaf to
    // every other.
    assert!(
        ours.addresses.iter().all(|a| a.ip.is_loopback()),
        "the loopback server has loopback addresses only: {ours:?}"
    );

    // Every answer reported is one: a server seen at an address that is not loopback, on an
    // interface that is not loopback, and reported before the answer was. Anything else on this
    // machine's network may answer while this test runs, and that is allowed; claiming an answer
    // without one is not.
    let mut off_loopback = false;
    for e in &all {
        if let Some(s) = e.server() {
            off_loopback |= s.addresses.iter().any(|a| {
                !a.ip.is_loopback() && a.interfaces.iter().any(|i| !i.name.starts_with("lo"))
            });
        }
        if is(e, &heard) {
            assert!(
                off_loopback,
                "an answer from the network was reported with nothing heard off loopback: {all:?}"
            );
        }
    }

    // What follows holds only where nothing answers, which is the state this is all about: a
    // machine whose own network answers settles the question by itself, as it should, and there
    // is nothing left to ask, however the sends of this process fare. The asking of a machine
    // that hears nothing is held without a network by the unit tests of `Asking` in src/mdns.rs.
    if !all.iter().any(|e| is(e, &heard)) {
        // It asked again while refused, rather than deciding once at the start.
        let times = asked.load(std::sync::atomic::Ordering::Relaxed);
        assert!(
            times >= (REFUSED_FOR.as_secs_f64() / ASK_EVERY.as_secs_f64()) as usize,
            "the system is asked again while it refuses, not once at the start: asked {times} times"
        );
        assert!(
            all.iter()
                .any(|e| is(e, &oscq_discovery::LocalNetwork::Asked)),
            "a query the system takes is reported as taken, not as an answer: {all:?}"
        );
        assert!(
            all.iter()
                .any(|e| is(e, &oscq_discovery::LocalNetwork::Unanswered)),
            "queries taken with nothing answering are reported as unanswered: {all:?}"
        );
        let before = asked.load(std::sync::atomic::Ordering::Relaxed);
        std::thread::sleep(ASK_EVERY * 4);
        assert!(
            asked.load(std::sync::atomic::Ordering::Relaxed) > before,
            "the asking stopped although nothing had answered: asked {before} times and no more"
        );
    } else {
        eprintln!(
            "this machine's own network answered: the unanswered half is held by the unit tests"
        );
    }
    discovery.stop();
    let _ = publisher.shutdown();
}

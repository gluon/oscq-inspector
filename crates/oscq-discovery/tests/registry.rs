//! Deduplication, stale marking and address policy, fed with observations shaped like real
//! advertisements.

#![allow(clippy::unwrap_used)] // test code, unwrap is allowed outside production paths (PRD 11)

use std::net::IpAddr;

use oscq_core::{AddressClass, InterfaceRef, Origin, ServiceKind};
use oscq_discovery::registry::{Observation, Registry};
use oscq_discovery::{DiscoveryEvent, OSC_SERVICE, OSCQUERY_SERVICE};

const HELPER: &str = "OSCQuery Helper._oscjson._tcp.local.";

fn iface(name: &str, index: u32) -> InterfaceRef {
    InterfaceRef {
        name: name.into(),
        index,
    }
}

fn obs(fullname: &str, host: &str, port: u16, addrs: &[(&str, InterfaceRef)]) -> Observation {
    Observation {
        kind: ServiceKind::OscQuery,
        service_type: OSCQUERY_SERVICE.into(),
        fullname: fullname.into(),
        host: host.into(),
        port,
        addresses: addrs
            .iter()
            .map(|(ip, i)| (ip.parse::<IpAddr>().unwrap(), vec![i.clone()]))
            .collect(),
    }
}

/// The real capture: OSCQuery Helper announced twice, on interfaces 1 and 14, same instance
/// name with spaces, same host and port. One entry, both interfaces kept.
#[test]
fn helper_announced_on_two_interfaces_is_one_server() {
    let mut r = Registry::new();
    let first = r
        .observe(obs(
            HELPER,
            "Studio-Mac.local.",
            2345,
            &[("127.0.0.1", iface("lo0", 1))],
        ))
        .unwrap();
    let DiscoveryEvent::Found(found) = first else {
        panic!("{first:?}");
    };
    let second = r
        .observe(obs(
            HELPER,
            "Studio-Mac.local.",
            2345,
            &[("192.168.1.20", iface("en0", 14))],
        ))
        .unwrap();
    let DiscoveryEvent::Changed(changed) = second else {
        panic!("{second:?}");
    };
    assert_eq!(changed.id, found.id);
    assert_eq!(r.servers().count(), 1);

    let s = r.servers().next().unwrap();
    assert_eq!(s.name, "OSCQuery Helper");
    assert_eq!(
        s.origin,
        Origin::Mdns {
            instance: "OSCQuery Helper".into(),
            service_type: OSCQUERY_SERVICE.into()
        }
    );
    assert_eq!((s.host.as_str(), s.port), ("studio-mac.local", 2345));
    let summary: Vec<(String, AddressClass, Vec<u32>)> = s
        .addresses
        .iter()
        .map(|a| {
            (
                a.ip.to_string(),
                a.class,
                a.interfaces.iter().map(|i| i.index).collect(),
            )
        })
        .collect();
    assert_eq!(
        summary,
        vec![
            ("192.168.1.20".into(), AddressClass::Ipv4, vec![14]),
            ("127.0.0.1".into(), AddressClass::LoopbackOrBridge, vec![1]),
        ]
    );
    assert_eq!(s.http_base().unwrap().as_str(), "http://192.168.1.20:2345/");

    // The same announcement again changes nothing and emits nothing.
    assert_eq!(
        r.observe(obs(
            HELPER,
            "Studio-Mac.local.",
            2345,
            &[("192.168.1.20", iface("en0", 14))]
        )),
        None
    );
}

#[test]
fn deduplication_is_by_host_and_port_not_instance_name() {
    let mut r = Registry::new();
    r.observe(obs(
        HELPER,
        "studio-mac.local.",
        2345,
        &[("10.0.0.2", iface("en0", 4))],
    ));
    let e = r.observe(obs(
        "Renamed Helper._oscjson._tcp.local.",
        "STUDIO-MAC.local",
        2345,
        &[("10.0.0.2", iface("en0", 4))],
    ));
    assert_eq!(e, None, "same host and port, other name: same server");
    assert_eq!(r.servers().count(), 1);
}

/// PRD 11: two servers advertising the same instance name.
#[test]
fn same_instance_name_on_two_hosts_is_two_servers() {
    let mut r = Registry::new();
    r.observe(obs(
        HELPER,
        "studio-a.local.",
        2345,
        &[("10.0.0.2", iface("en0", 4))],
    ));
    r.observe(obs(
        HELPER,
        "studio-b.local.",
        2345,
        &[("10.0.0.3", iface("en0", 4))],
    ));
    assert_eq!(r.servers().count(), 2);
    let names: Vec<&str> = r.servers().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["OSCQuery Helper", "OSCQuery Helper"]);
}

/// PRD 11: server disappearing from mDNS while connected. Stale, kept, same id when it returns.
#[test]
fn vanished_server_is_stale_then_revives() {
    let mut r = Registry::new();
    let Some(DiscoveryEvent::Found(found)) = r.observe(obs(
        HELPER,
        "studio.local.",
        2345,
        &[("10.0.0.2", iface("en0", 4))],
    )) else {
        panic!();
    };
    let events = r.source_removed(HELPER);
    assert_eq!(events.len(), 1);
    let DiscoveryEvent::Stale(stale) = &events[0] else {
        panic!("{events:?}");
    };
    assert!(stale.stale);
    assert_eq!(stale.id, found.id);
    assert_eq!(r.servers().count(), 1, "kept for retry");
    assert!(r.source_removed(HELPER).is_empty(), "no second stale event");

    let Some(DiscoveryEvent::Changed(back)) = r.observe(obs(
        HELPER,
        "studio.local.",
        2345,
        &[("10.0.0.2", iface("en0", 4))],
    )) else {
        panic!();
    };
    assert!(!back.stale);
    assert_eq!(back.id, found.id);
}

#[test]
fn stale_only_when_every_advertisement_is_gone() {
    let mut r = Registry::new();
    r.observe(obs(
        HELPER,
        "studio.local.",
        2345,
        &[("10.0.0.2", iface("en0", 4))],
    ));
    r.observe(obs(
        "Other Name._oscjson._tcp.local.",
        "studio.local.",
        2345,
        &[("10.0.0.2", iface("en0", 4))],
    ));
    assert!(r.source_removed(HELPER).is_empty());
    assert!(!r.servers().next().unwrap().stale);
    assert_eq!(r.source_removed("Other Name._oscjson._tcp.local.").len(), 1);
    assert!(r.servers().next().unwrap().stale);
}

#[test]
fn unknown_removal_is_ignored() {
    let mut r = Registry::new();
    assert!(r.source_removed("Nobody._oscjson._tcp.local.").is_empty());
}

/// PRD 11: IPv6 only advertisement.
#[test]
fn ipv6_only_advertisement() {
    let mut r = Registry::new();
    let Some(DiscoveryEvent::Found(s)) = r.observe(obs(
        HELPER,
        "v6.local.",
        2345,
        &[("fe80::1", iface("en0", 4)), ("fd00::2", iface("en0", 4))],
    )) else {
        panic!();
    };
    let classes: Vec<AddressClass> = s.addresses.iter().map(|a| a.class).collect();
    assert_eq!(classes, [AddressClass::Ipv6, AddressClass::Ipv6LinkLocal]);
    assert_eq!(s.http_base().unwrap().as_str(), "http://[fd00::2]:2345/");
}

#[test]
fn docker_bridge_and_loopback_are_fallbacks_only() {
    let mut r = Registry::new();
    let Some(DiscoveryEvent::Found(s)) = r.observe(obs(
        HELPER,
        "box.local.",
        2345,
        &[
            ("172.17.0.1", iface("docker0", 3)),
            ("127.0.0.1", iface("lo", 1)),
        ],
    )) else {
        panic!();
    };
    assert_eq!(s.addresses.len(), 2, "nothing discarded");
    assert!(
        s.addresses
            .iter()
            .all(|a| a.class == AddressClass::LoopbackOrBridge)
    );
    assert_eq!(s.http_base().unwrap().as_str(), "http://127.0.0.1:2345/");
}

#[test]
fn osc_only_service_is_listed_but_not_connectable() {
    let mut r = Registry::new();
    let osc = Observation {
        kind: ServiceKind::OscOnly,
        service_type: OSC_SERVICE.into(),
        fullname: "Lighting Desk._osc._udp.local.".into(),
        host: "desk.local.".into(),
        port: 8000,
        addresses: vec![("10.0.0.9".parse().unwrap(), vec![iface("en0", 4)])],
    };
    let Some(DiscoveryEvent::Found(s)) = r.observe(osc) else {
        panic!();
    };
    assert_eq!(s.kind, ServiceKind::OscOnly);
    assert_eq!(s.name, "Lighting Desk");
    assert!(!s.kind.connectable());
    assert_eq!(s.http_base(), None);

    // Same host and port over OSCQuery is a different service, not merged.
    r.observe(obs(
        HELPER,
        "desk.local.",
        8000,
        &[("10.0.0.9", iface("en0", 4))],
    ));
    assert_eq!(r.servers().count(), 2);
}

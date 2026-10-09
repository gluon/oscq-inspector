//! Real documents captured from Vidvox OSCQuery Helper, HTTP port 2345, OSC target port 1240.

#![allow(clippy::unwrap_used)] // test code, unwrap is allowed outside production paths (PRD 11)

mod common;

use common::read_fixture;
use oscq_core::types::{ArgType, TypeTag};
use oscq_core::{Access, ArgEntry, HostInfo, OscTransport, PortSource, parse_document};
use serde_json::json;

fn root() -> oscq_core::ParseReport {
    parse_document(&read_fixture("helper-root.json")).unwrap()
}

#[test]
fn whole_tree_parses_without_diagnostics() {
    let r = root();
    assert!(r.diagnostics.is_empty(), "{:?}", r.diagnostics);
    assert_eq!(r.root.count(), 22);
    assert!(r.root.find("/test/my_float").unwrap().path.is_some());
}

#[test]
fn nested_type_signatures() {
    use ArgType::{Array, Tag};
    use TypeTag::*;
    let r = root();
    let sig = |p: &str| r.root.find(p).unwrap().signature.clone().unwrap();
    assert_eq!(
        sig("/tuples/first").args(),
        &[Tag(Int32), Array(vec![Tag(Int32), Tag(Int32)]), Tag(Int32)]
    );
    assert_eq!(
        sig("/tuples/second").args(),
        &[
            Tag(Int32),
            Array(vec![Tag(String), Tag(Float32)]),
            Tag(Double),
            Tag(Char)
        ]
    );
    assert_eq!(sig("/vecs/first").args().len(), 4);
    assert_eq!(sig("/test/my_midi").args(), &[Tag(Midi)]);
    assert_eq!(sig("/test/timetag").args(), &[Tag(TimeTag)]);
}

/// The Helper declares `ACCESS` 2 on every node: it forwards to an OSC target whose address
/// space it cannot read. An explicit `ACCESS` always wins: every node is reported write only,
/// even though `EXTENSIONS` lists `VALUE`, and even on the nodes that carry a `VALUE`.
#[test]
fn every_node_is_write_only_as_declared() {
    let r = root();
    let (host, _) = HostInfo::parse(&read_fixture("helper-hostinfo.json")).unwrap();
    let value_supported = host.supports("VALUE");
    assert!(value_supported);
    let mut stack = vec![&r.root];
    let mut seen = 0;
    let mut with_value = 0;
    while let Some(n) = stack.pop() {
        assert_eq!(n.access, Some(Access::WRITE), "{}", n.full_path);
        let effective = n.effective_access(value_supported);
        assert_eq!(effective, Access::WRITE, "{}", n.full_path);
        assert!(
            !effective.readable() && effective.writable(),
            "{}",
            n.full_path
        );
        seen += 1;
        with_value += usize::from(n.value.is_some());
        stack.extend(n.children());
    }
    assert_eq!(seen, 22);
    assert_eq!(with_value, 2, "my_true and my_false carry a VALUE");
}

#[test]
fn boolean_nodes_carry_their_value() {
    let r = root();
    let value = |p: &str| r.root.find(p).unwrap().value.clone().unwrap();
    assert_eq!(
        value("/test/my_true").entry(0),
        Some(&ArgEntry::Value(json!(true)))
    );
    assert_eq!(
        value("/test/my_false").entry(0),
        Some(&ArgEntry::Value(json!(false)))
    );
}

/// No `OSC_IP`, no `WS_IP`, no `WS_PORT`, no WebSocket extension: the nominal Helper case.
#[test]
fn host_info_nominal_case() {
    let (h, diags) = HostInfo::parse(&read_fixture("helper-hostinfo.json")).unwrap();
    assert!(diags.is_empty(), "{diags:?}");
    assert_eq!(h.name.as_deref(), Some("SampleDocument"));
    assert!(h.extra.is_empty());

    let d = h.osc_destination("192.168.1.10", 2345).unwrap();
    assert_eq!(d.host, "192.168.1.10");
    assert_eq!((d.port, d.port_source), (1240, PortSource::Advertised));
    assert_eq!(d.transport, OscTransport::Udp);
    assert!(d.check_sendable().is_ok());

    assert_eq!(
        h.websocket_address("192.168.1.10", 2345),
        ("192.168.1.10".to_owned(), 2345, false)
    );
    for ext in ["RANGE", "ACCESS", "HTML", "PATH_CHANGED", "TAGS", "VALUE"] {
        assert!(h.supports(ext), "{ext}");
    }
    assert!(!h.supports("LISTEN"));
}

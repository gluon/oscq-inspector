//! Real documents captured from Max 9, which serves OSCQuery natively on HTTP port 30339, from
//! the patcher in `tests/fixtures/max/`.

#![allow(clippy::unwrap_used)] // test code, unwrap is allowed outside production paths (PRD 11)

mod common;

use common::read_fixture;
use oscq_core::types::{ArgType, TypeTag};
use oscq_core::{
    Access, ArgEntry, DiagnosticKind, HostInfo, Node, OscTransport, PortSource, parse_document,
};
use serde_json::json;

fn root() -> oscq_core::ParseReport {
    parse_document(&read_fixture("max-root.json")).unwrap()
}

fn all(root: &Node) -> Vec<&Node> {
    let mut out = Vec::new();
    let mut stack = vec![root];
    while let Some(n) = stack.pop() {
        out.push(n);
        stack.extend(n.children());
    }
    out
}

/// Three parameters, each attribute of a parameter a node of its own, five levels from the root.
#[test]
fn whole_tree_parses_without_diagnostics() {
    let r = root();
    assert!(r.diagnostics.is_empty(), "{:?}", r.diagnostics);
    assert_eq!(r.root.count(), 36);
    let containers: Vec<&str> = all(&r.root)
        .into_iter()
        .filter(|n| n.is_container())
        .map(|n| n.full_path.as_str())
        .collect();
    assert_eq!(
        containers.len(),
        6,
        "/, /test, /test/param and three parameters"
    );
    for p in ["gain", "freq", "fader"] {
        let param = r.root.find(&format!("/test/param/{p}")).unwrap();
        let names: Vec<&str> = param
            .contents
            .as_ref()
            .unwrap()
            .iter()
            .map(|(k, _)| k.as_str())
            .collect();
        assert_eq!(
            names,
            [
                "exponent",
                "longname",
                "max",
                "min",
                "normalized",
                "raw",
                "scriptname",
                "shortname",
                "type",
                "visibility"
            ],
            "{p}"
        );
    }
    let deepest = all(&r.root)
        .into_iter()
        .map(|n| n.full_path.matches('/').count())
        .max();
    assert_eq!(
        deepest,
        Some(4),
        "/test/param/gain/raw is four levels below the root"
    );
}

/// Max declares `ACCESS` on every method and lists no `EXTENSIONS`: the explicit `ACCESS` wins,
/// so read only and read write nodes are readable without `VALUE` being advertised.
#[test]
fn readable_nodes_with_real_values() {
    let r = root();
    let mut read_only = 0;
    let mut read_write = 0;
    for n in all(&r.root).into_iter().filter(|n| n.is_method()) {
        let effective = n.effective_access(false);
        assert_eq!(n.access, Some(effective), "{}", n.full_path);
        assert!(effective.readable(), "{}", n.full_path);
        assert!(n.value.is_some(), "{}", n.full_path);
        if effective == Access::READ {
            read_only += 1;
        } else {
            assert_eq!(effective, Access::READ_WRITE, "{}", n.full_path);
            read_write += 1;
        }
    }
    assert_eq!((read_only, read_write), (24, 6));

    let node = |p: &str| r.root.find(p).unwrap();
    let value = |p: &str| node(p).value.clone().unwrap().entry(0).cloned();
    assert_eq!(
        value("/test/param/fader/normalized"),
        Some(ArgEntry::Value(json!(0.5)))
    );
    assert_eq!(
        value("/test/param/fader/min"),
        Some(ArgEntry::Value(json!(-1000.0)))
    );
    assert_eq!(
        value("/test/param/freq/longname"),
        Some(ArgEntry::Value(json!("freq")))
    );
    assert_eq!(
        node("/test/param/gain/raw")
            .signature
            .as_ref()
            .unwrap()
            .args(),
        &[ArgType::Tag(TypeTag::Double)]
    );
    assert_eq!(
        node("/test/param/gain/raw").access,
        Some(Access::READ_WRITE)
    );
    assert_eq!(node("/test/param/gain/max").access, Some(Access::READ));
}

/// Max ignores the `HOST_INFO` query and returns its whole tree: the two captures are the same
/// bytes. The reply is detected as an address space document, not a `HOST_INFO`, and every
/// default applies without an error: OSC and WebSocket on the HTTP address and port.
#[test]
fn host_info_query_ignored_falls_back_to_the_http_port() {
    let body = read_fixture("max-hostinfo.json");
    assert_eq!(body, read_fixture("max-root.json"), "identical captures");
    let (h, diags) = HostInfo::parse(&body).unwrap();
    assert!(h.query_ignored);
    assert_eq!(
        diags.iter().map(|d| d.kind.clone()).collect::<Vec<_>>(),
        [DiagnosticKind::HostInfoIgnored]
    );
    assert_eq!(
        h,
        HostInfo {
            query_ignored: true,
            ..HostInfo::default()
        },
        "nothing of the tree is taken for HOST_INFO keys"
    );

    let d = h.osc_destination("127.0.0.1", 30339).unwrap();
    assert_eq!(d.host, "127.0.0.1");
    assert_eq!(
        (d.port, d.port_source),
        (30339, PortSource::InferredFromHttp)
    );
    assert_eq!(d.transport, OscTransport::Udp);
    assert!(d.check_sendable().is_ok());
    assert_eq!(
        h.websocket_address("127.0.0.1", 30339),
        ("127.0.0.1".to_owned(), 30339, false),
        "no WebSocket advertised: one attempt on the HTTP address"
    );
    assert!(!h.supports("VALUE") && !h.supports("LISTEN"));
}

/// A real `HOST_INFO` is never mistaken for a document.
#[test]
fn a_real_host_info_is_not_flagged() {
    let (h, diags) = HostInfo::parse(&read_fixture("helper-hostinfo.json")).unwrap();
    assert!(!h.query_ignored);
    assert!(diags.is_empty());
}

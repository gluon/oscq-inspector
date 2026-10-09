//! Parsing of every committed fixture and of generated edge cases.

#![allow(clippy::unwrap_used)] // test code, unwrap is allowed outside production paths (PRD 11)

mod common;

use common::{deep_document, large_document, read_fixture};
use oscq_core::document::{MAX_JSON_DEPTH, MAX_NODE_DEPTH};
use oscq_core::model::{parse_rgba_hex, rgba_color_component};
use oscq_core::{
    Access, ArgEntry, ClipMode, DiagnosticKind, DocumentError, HostInfo, Node, OscTransport,
    PerArg, PortSource, SendBlocked, parse_document,
};
use serde_json::json;

fn doc(rel: &str) -> oscq_core::ParseReport {
    parse_document(&read_fixture(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

fn node<'a>(root: &'a Node, path: &str) -> &'a Node {
    root.find(path).unwrap_or_else(|| panic!("no node {path}"))
}

fn kinds_at(report: &oscq_core::ParseReport, path: &str) -> Vec<DiagnosticKind> {
    report
        .diagnostics
        .iter()
        .filter(|d| d.path == path)
        .map(|d| d.kind.clone())
        .collect()
}

#[test]
fn basic_tree() {
    let r = doc("documents/basic_tree.json");
    assert!(r.diagnostics.is_empty(), "{:?}", r.diagnostics);
    assert_eq!(r.root.count(), 5);
    assert_eq!(r.root.access, Some(Access::NONE));
    assert_eq!(node(&r.root, "/foo").access, Some(Access::READ));
    assert_eq!(
        node(&r.root, "/bar").signature.as_ref().unwrap().as_str(),
        "ii"
    );
    let qux = node(&r.root, "/baz/qux");
    assert_eq!(qux.critical, Some(true));
    assert_eq!(qux.tags.as_deref(), Some(&["level".to_owned()][..]));
    let Some(ArgEntry::Value(range)) = qux.range.as_ref().and_then(|r| r.entry(0)) else {
        panic!("range");
    };
    assert_eq!(range.vals.as_ref().map(Vec::len), Some(3));
    assert_eq!(
        qux.extra.get("X_VENDOR_KEY"),
        Some(&json!({"kept": "verbatim"}))
    );
    assert!(node(&r.root, "/baz").is_container());
}

#[test]
fn per_argument_nested_arrays() {
    let r = doc("documents/per_arg_nested.json");
    assert!(r.diagnostics.is_empty(), "{:?}", r.diagnostics);
    let n = &r.root;
    assert_eq!(
        n.value,
        Some(PerArg::List(vec![
            ArgEntry::Value(json!(100)),
            ArgEntry::Nested(vec![
                ArgEntry::Value(json!(1.0)),
                ArgEntry::Value(json!(1.0))
            ]),
            ArgEntry::Value(json!(100)),
        ]))
    );
    let Some(ArgEntry::Nested(inner)) = n.range.as_ref().and_then(|r| r.entry(1)) else {
        panic!("nested range");
    };
    assert_eq!(inner.len(), 2);
    assert_eq!(
        n.clipmode.as_ref().and_then(|c| c.entry(1)),
        Some(&ArgEntry::Nested(vec![
            ArgEntry::Value(ClipMode::Low),
            ArgEntry::Value(ClipMode::High)
        ]))
    );
    assert_eq!(
        n.unit.as_ref().and_then(|u| u.entry(2)),
        Some(&ArgEntry::Value("distance.mm".to_owned()))
    );
    assert!(matches!(
        n.extended_type.as_ref().and_then(|e| e.entry(1)),
        Some(ArgEntry::Nested(_))
    ));
}

#[test]
fn per_argument_null_entries() {
    let r = doc("documents/per_arg_null.json");
    assert!(r.diagnostics.is_empty(), "{:?}", r.diagnostics);
    let n = &r.root;
    assert_eq!(
        n.value.as_ref().and_then(|v| v.entry(1)),
        Some(&ArgEntry::Null)
    );
    assert_eq!(
        n.range.as_ref().and_then(|v| v.entry(1)),
        Some(&ArgEntry::Null)
    );
    assert_eq!(
        n.unit.as_ref().and_then(|v| v.entry(1)),
        Some(&ArgEntry::Null)
    );
    assert_eq!(
        n.extended_type.as_ref().and_then(|v| v.entry(0)),
        Some(&ArgEntry::Null)
    );
    assert_eq!(
        n.clipmode.as_ref().and_then(|v| v.entry(1)),
        Some(&ArgEntry::Null)
    );
    assert_eq!(
        n.clipmode.as_ref().and_then(|v| v.entry(0)),
        Some(&ArgEntry::Value(ClipMode::Both))
    );
}

#[test]
fn per_argument_single_scalar() {
    let r = doc("documents/per_arg_scalar.json");
    assert!(r.diagnostics.is_empty(), "{:?}", r.diagnostics);
    let n = &r.root;
    for i in 0..3 {
        assert_eq!(
            n.value.as_ref().and_then(|v| v.entry(i)),
            Some(&ArgEntry::Value(json!(0.25)))
        );
        assert_eq!(
            n.clipmode.as_ref().and_then(|v| v.entry(i)),
            Some(&ArgEntry::Value(ClipMode::Both))
        );
        assert_eq!(
            n.unit.as_ref().and_then(|v| v.entry(i)),
            Some(&ArgEntry::Value("gain.linear".to_owned()))
        );
        let Some(ArgEntry::Value(range)) = n.range.as_ref().and_then(|v| v.entry(i)) else {
            panic!("range");
        };
        assert_eq!(
            (range.min.clone(), range.max.clone()),
            (Some(json!(0.0)), Some(json!(1.0)))
        );
    }
    assert_eq!(n.value.as_ref().and_then(PerArg::len), None);
}

#[test]
fn node_without_contents_is_a_method_even_without_type() {
    let r = doc("documents/method_without_type.json");
    assert!(r.diagnostics.is_empty(), "{:?}", r.diagnostics);
    let bang = node(&r.root, "/bang");
    assert!(bang.is_method() && !bang.is_container());
    assert!(bang.signature.is_none());
    let group = node(&r.root, "/group");
    assert!(group.is_container() && group.is_method());
    let empty = node(&r.root, "/empty_container");
    assert!(empty.is_container() && !empty.is_method());
    assert!(r.root.is_container() && !r.root.is_method());
}

#[test]
fn access_absent_zero_and_invalid() {
    let r = doc("documents/access_absent.json");
    let level = node(&r.root, "/level");
    assert_eq!(level.access, None);
    assert_eq!(level.effective_access(true), Access::READ_WRITE);
    assert_eq!(level.effective_access(false), Access::WRITE);
    let silent = node(&r.root, "/silent");
    assert_eq!(silent.effective_access(true), Access::NONE);
    let bad = node(&r.root, "/bad_access");
    assert_eq!(bad.access, None);
    assert_eq!(bad.extra.get("ACCESS"), Some(&json!(9)));
    assert_eq!(
        kinds_at(&r, "/bad_access"),
        vec![DiagnosticKind::InvalidAccess]
    );
}

#[test]
fn type_and_value_length_disagree() {
    let r = doc("documents/value_length_mismatch.json");
    assert_eq!(r.root.count(), 4);
    assert_eq!(
        kinds_at(&r, "/short"),
        vec![DiagnosticKind::ValueLengthMismatch {
            declared: 3,
            received: 1
        }]
    );
    assert_eq!(
        kinds_at(&r, "/long"),
        vec![DiagnosticKind::ValueLengthMismatch {
            declared: 1,
            received: 3
        }]
    );
    assert!(kinds_at(&r, "/untyped").is_empty());
    assert_eq!(
        node(&r.root, "/long").value.as_ref().and_then(PerArg::len),
        Some(3)
    );
}

#[test]
fn paths_legal_in_json_but_not_in_osc() {
    let r = doc("documents/invalid_osc_paths.json");
    assert_eq!(r.root.count(), 7, "nodes are kept");
    for p in [
        "/with space",
        "/hash#tag",
        "/star*",
        "/caf\u{e9}",
        "/ctrl\u{1}",
    ] {
        let n = node(&r.root, p);
        assert!(n.path.is_none(), "{p}");
        assert!(
            matches!(
                kinds_at(&r, p).as_slice(),
                [DiagnosticKind::InvalidOscPath(_)]
            ),
            "{p}"
        );
    }
    assert!(node(&r.root, "/ok").path.is_some());
}

#[test]
fn colours() {
    let r = doc("documents/colors.json");
    assert!(r.diagnostics.is_empty(), "{:?}", r.diagnostics);
    let native = node(&r.root, "/native");
    assert_eq!(native.signature.as_ref().unwrap().as_str(), "r");
    assert!(native.extended_type.is_none());
    let Some(ArgEntry::Value(v)) = native.value.as_ref().and_then(|v| v.entry(0)) else {
        panic!("value");
    };
    assert_eq!(
        v.as_str().and_then(parse_rgba_hex),
        Some([250, 100, 50, 255])
    );

    let components = |n: &Node| -> Vec<Option<char>> {
        (0..4)
            .map(
                |i| match n.extended_type.as_ref().and_then(|e| e.entry(i)) {
                    Some(ArgEntry::Value(s)) => rgba_color_component(s),
                    _ => None,
                },
            )
            .collect()
    };
    let rgba = vec![Some('r'), Some('g'), Some('b'), Some('a')];
    assert_eq!(components(node(&r.root, "/rgb_spelling")), rgba);
    let overloads = node(&r.root, "/rgba_spelling").overloads.as_ref().unwrap();
    assert_eq!(overloads.len(), 2);
    assert_eq!(components(&overloads[0]), rgba);
    assert_eq!(overloads[1].signature.as_ref().unwrap().as_str(), "iiii");
}

#[test]
fn no_payload_tags_and_unknown_tag() {
    let r = doc("documents/no_payload_tags.json");
    let toggle = node(&r.root, "/toggle");
    assert_eq!(
        toggle.value.as_ref().and_then(|v| v.entry(0)),
        Some(&ArgEntry::Value(json!(true)))
    );
    for p in ["/impulse", "/nil"] {
        assert_eq!(
            node(&r.root, p).value.as_ref().and_then(|v| v.entry(0)),
            Some(&ArgEntry::Null)
        );
    }
    let bad = node(&r.root, "/bad_type");
    assert!(bad.signature.is_none());
    assert_eq!(bad.type_raw.as_deref(), Some("fx"));
    assert!(matches!(
        kinds_at(&r, "/bad_type").as_slice(),
        [DiagnosticKind::InvalidType(_)]
    ));
}

#[test]
fn missing_full_path_is_derived() {
    let r = doc("documents/missing_full_path.json");
    assert_eq!(r.root.full_path, "/");
    assert!(node(&r.root, "/a/b").path.is_some());
    assert_eq!(
        kinds_at(&r, "/"),
        vec![
            DiagnosticKind::MissingFullPath,
            DiagnosticKind::InvalidChild("c".into())
        ]
    );
    assert_eq!(kinds_at(&r, "/a"), vec![DiagnosticKind::MissingFullPath]);
    assert_eq!(kinds_at(&r, "/a/b"), vec![DiagnosticKind::MissingFullPath]);
}

#[test]
fn bodies_that_are_not_address_spaces() {
    assert!(matches!(
        parse_document(&read_fixture("documents/not_json.txt")),
        Err(DocumentError::NotJson(_))
    ));
    assert!(matches!(
        parse_document(&read_fixture("documents/not_an_object.json")),
        Err(DocumentError::NotAnObject)
    ));
    assert!(matches!(
        parse_document(&read_fixture("documents/not_an_address_space.json")),
        Err(DocumentError::NotAnAddressSpace)
    ));
}

#[test]
fn vendor_sample_document_is_not_valid_json() {
    assert!(matches!(
        parse_document(&read_fixture(
            "third_party/vvoscqueryprotocol_helper_sample_document.json"
        )),
        Err(DocumentError::NotJson(_))
    ));
}

#[test]
fn five_thousand_nodes() {
    let r = parse_document(large_document(5000).as_bytes()).unwrap();
    assert!(r.diagnostics.is_empty());
    assert_eq!(r.root.count(), 1 + 50 + 5000);
}

#[test]
fn depth_capped_at_64_and_reported() {
    let r = parse_document(deep_document(70).as_bytes()).unwrap();
    assert_eq!(r.root.count(), MAX_NODE_DEPTH + 1);
    let limit: Vec<_> = r
        .diagnostics
        .iter()
        .filter(|d| d.kind == DiagnosticKind::DepthLimit)
        .collect();
    assert_eq!(limit.len(), 1);
    assert_eq!(limit[0].path.matches('/').count(), MAX_NODE_DEPTH);
}

#[test]
fn hostile_nesting_is_rejected_without_recursion() {
    let bomb = "[".repeat(1_000_000);
    assert!(matches!(
        parse_document(bomb.as_bytes()),
        Err(DocumentError::TooDeep)
    ));
    let just_over = format!(
        "{}{}",
        "[".repeat(MAX_JSON_DEPTH + 1),
        "]".repeat(MAX_JSON_DEPTH + 1)
    );
    assert!(matches!(
        parse_document(just_over.as_bytes()),
        Err(DocumentError::TooDeep)
    ));
}

#[test]
fn deepest_accepted_json_fits_a_small_stack() {
    // The Windows main thread has 1 MiB of stack; parse at the depth limit within half of it.
    let depth = MAX_JSON_DEPTH;
    let body = format!(
        r#"{{"FULL_PATH":"/","TYPE":"f","X":{}0{}}}"#,
        "[".repeat(depth - 1),
        "]".repeat(depth - 1)
    );
    let handle = std::thread::Builder::new()
        .stack_size(512 * 1024)
        .spawn(move || parse_document(body.as_bytes()).map(|r| r.root.count()))
        .unwrap();
    assert_eq!(handle.join().unwrap().unwrap(), 1);
}

fn host(rel: &str) -> HostInfo {
    HostInfo::parse(&read_fixture(rel)).unwrap().0
}

#[test]
fn host_info_full() {
    let h = host("host_info/full.json");
    assert_eq!(h.name.as_deref(), Some("Synthetic Server"));
    let d = h.osc_destination("10.0.0.2", 5678).unwrap();
    assert_eq!(
        (d.host.as_str(), d.port, d.port_source),
        ("127.0.0.1", 9001, PortSource::Advertised)
    );
    assert_eq!(d.transport, OscTransport::Udp);
    assert_eq!(
        h.websocket_address("10.0.0.2", 5678),
        ("127.0.0.1".to_owned(), 9002, true)
    );
    assert!(h.supports("VALUE") && h.supports("LISTEN"));
    assert!(!h.supports("HTML") && !h.supports("PATH_RENAMED"));
}

#[test]
fn host_info_osc_port_absent_uses_http_port() {
    let h = host("host_info/no_osc_port.json");
    let d = h.osc_destination("10.0.0.2", 5678).unwrap();
    assert_eq!(
        (d.host.as_str(), d.port, d.port_source),
        ("10.0.0.2", 5678, PortSource::InferredFromHttp)
    );
    assert!(d.check_sendable().is_ok());
}

#[test]
fn host_info_tcp_blocks_sending() {
    let d = host("host_info/tcp.json").osc_destination("h", 1).unwrap();
    assert_eq!(d.transport, OscTransport::Tcp);
    assert_eq!(d.check_sendable(), Err(SendBlocked::TcpTransport));
}

#[test]
fn host_info_ports_as_strings() {
    let h = host("host_info/string_port.json");
    assert_eq!(h.osc_port, Some(9001));
    assert_eq!(h.ws_port, Some(9002));
}

#[test]
fn host_info_invalid_port_is_not_controllable() {
    let (h, diags) = HostInfo::parse(&read_fixture("host_info/invalid_port.json")).unwrap();
    assert_eq!(h.osc_destination("h", 1), Err(SendBlocked::InvalidOscPort));
    assert_eq!(diags.len(), 1);
}

#[test]
fn host_info_empty_uses_every_default() {
    let h = host("host_info/empty.json");
    let d = h.osc_destination("10.0.0.2", 5678).unwrap();
    assert_eq!(
        (d.port, d.port_source, d.transport.clone()),
        (5678, PortSource::InferredFromHttp, OscTransport::Udp)
    );
    assert_eq!(
        h.websocket_address("10.0.0.2", 5678),
        ("10.0.0.2".to_owned(), 5678, false)
    );
    assert!(!h.supports("VALUE"));
}

//! The network service end to end against the synthetic server: commands in, events out.

#![allow(clippy::unwrap_used)] // test code, unwrap is allowed outside production paths (PRD 11)

mod common;

use std::sync::atomic::Ordering;
use std::time::Duration;

use common::{OscReceiver, Reply, SyntheticServer, harness, read_fixture};
use oscq_core::{
    AttributeReply, Command, Direction, Event, OscPath, PortSource, ServerId, TransportState,
    TypeSignature,
};
use rosc::{OscMessage, OscPacket, OscType};
use serde_json::json;

async fn helper_like(target_port: u16) -> SyntheticServer {
    let host_info =
        format!(r#"{{"NAME":"x","OSC_PORT":{target_port},"EXTENSIONS":{{"VALUE":true}}}}"#);
    SyntheticServer::start(
        "127.0.0.1:0",
        vec![
            ("/", Reply::json(read_fixture("documents/basic_tree.json"))),
            ("/?HOST_INFO", Reply::json(host_info)),
            ("/baz", Reply::json(r#"{"FULL_PATH":"/baz","CONTENTS":{}}"#)),
            ("/bar?VALUE", Reply::json(r#"{"VALUE":[7,60]}"#)),
        ],
    )
    .await
    .unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn connect_loads_host_info_then_tree() {
    let target = OscReceiver::bind("127.0.0.1:0").await.unwrap();
    let server = helper_like(target.port()).await;
    let h = harness();
    let id = ServerId::next();
    h.commands
        .send(Command::Connect {
            server_id: id,
            http_base: server.base_url(),
        })
        .unwrap();
    let seen = h
        .until(|e| {
            matches!(
                e,
                Event::TransportState {
                    state: TransportState::HttpOnly,
                    ..
                }
            )
        })
        .await;
    let dest = seen
        .iter()
        .find_map(|e| match e {
            Event::HostInfo { destination, .. } => Some(destination.clone().unwrap()),
            _ => None,
        })
        .unwrap();
    assert_eq!(
        (dest.port, dest.port_source),
        (target.port(), PortSource::Advertised)
    );
    let tree = seen
        .iter()
        .find_map(|e| match e {
            Event::TreeLoaded { raw, report, .. } => Some((raw.clone(), report.clone())),
            _ => None,
        })
        .unwrap();
    assert_eq!(tree.1.root.count(), 5);
    assert_eq!(
        tree.0.as_bytes(),
        read_fixture("documents/basic_tree.json").as_slice()
    );
    // notify runs right after each send, so it may lag the receiver by a moment.
    let deadline = std::time::Instant::now() + Duration::from_secs(1);
    while h.notified.load(Ordering::SeqCst) < seen.len() && std::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert!(
        h.notified.load(Ordering::SeqCst) >= seen.len(),
        "notify after every event"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn send_reaches_the_target_is_logged_and_refreshed() {
    let target = OscReceiver::bind("127.0.0.1:0").await.unwrap();
    let server = helper_like(target.port()).await;
    let h = harness();
    let id = ServerId::next();
    h.commands
        .send(Command::Connect {
            server_id: id,
            http_base: server.base_url(),
        })
        .unwrap();
    h.until(|e| {
        matches!(
            e,
            Event::TransportState {
                state: TransportState::HttpOnly,
                ..
            }
        )
    })
    .await;

    h.commands
        .send(Command::SendOsc {
            server_id: id,
            path: OscPath::parse("/bar").unwrap(),
            signature: TypeSignature::parse("ii").unwrap(),
            args: vec![OscType::Double(6.6), OscType::Long(60)],
            refresh_after: true,
        })
        .unwrap();
    assert_eq!(
        target.recv().await,
        OscPacket::Message(OscMessage {
            addr: "/bar".into(),
            args: vec![OscType::Int(7), OscType::Int(60)]
        })
    );
    let seen = h.until(|e| matches!(e, Event::ValueChanged { .. })).await;
    let traffic = seen
        .iter()
        .find_map(|e| match e {
            Event::TrafficLogged(t) => Some(t.clone()),
            _ => None,
        })
        .unwrap();
    assert_eq!(
        (
            traffic.direction,
            traffic.path.as_str(),
            traffic.type_tags.as_str()
        ),
        (Direction::Out, "/bar", "ii")
    );
    assert_eq!(traffic.args, vec![OscType::Int(7), OscType::Int(60)]);
    let Some(Event::ValueChanged { path, value, .. }) = seen.last() else {
        panic!();
    };
    assert_eq!(path, "/bar");
    assert_eq!(value, &AttributeReply::Value(json!([7, 60])));
}

#[tokio::test(flavor = "multi_thread")]
async fn refresh_subtree_and_value() {
    let target = OscReceiver::bind("127.0.0.1:0").await.unwrap();
    let server = helper_like(target.port()).await;
    let h = harness();
    let id = ServerId::next();
    h.commands
        .send(Command::Connect {
            server_id: id,
            http_base: server.base_url(),
        })
        .unwrap();
    h.until(|e| {
        matches!(
            e,
            Event::TransportState {
                state: TransportState::HttpOnly,
                ..
            }
        )
    })
    .await;
    h.commands
        .send(Command::RefreshTree {
            server_id: id,
            from_path: "/baz".into(),
        })
        .unwrap();
    let seen = h.until(|e| matches!(e, Event::SubtreeLoaded { .. })).await;
    let Some(Event::SubtreeLoaded { path, report, .. }) = seen.last() else {
        panic!();
    };
    assert_eq!(
        (path.as_str(), report.root.full_path.as_str()),
        ("/baz", "/baz")
    );
    h.commands
        .send(Command::RefreshValue {
            server_id: id,
            path: "/nothing".into(),
        })
        .unwrap();
    let seen = h.until(|e| matches!(e, Event::Error { .. })).await;
    let Some(Event::Error { error, .. }) = seen.last() else {
        panic!();
    };
    assert_eq!(error.path.as_deref(), Some("/nothing"));
    assert!(error.message.contains("404"), "{}", error.message);
}

#[tokio::test(flavor = "multi_thread")]
async fn no_host_info_is_browsable_but_send_is_refused_with_reason() {
    let server = SyntheticServer::start(
        "127.0.0.1:0",
        vec![("/", Reply::json(read_fixture("documents/basic_tree.json")))],
    )
    .await
    .unwrap();
    let h = harness();
    let id = ServerId::next();
    h.commands
        .send(Command::Connect {
            server_id: id,
            http_base: server.base_url(),
        })
        .unwrap();
    let seen = h
        .until(|e| {
            matches!(
                e,
                Event::TransportState {
                    state: TransportState::HttpOnly,
                    ..
                }
            )
        })
        .await;
    assert!(
        seen.iter()
            .any(|e| matches!(e, Event::HostInfoUnavailable { .. }))
    );
    assert!(seen.iter().any(|e| matches!(e, Event::TreeLoaded { .. })));
    h.commands
        .send(Command::SendOsc {
            server_id: id,
            path: OscPath::parse("/bar").unwrap(),
            signature: TypeSignature::parse("ii").unwrap(),
            args: vec![OscType::Int(1), OscType::Int(2)],
            refresh_after: false,
        })
        .unwrap();
    let seen = h.until(|e| matches!(e, Event::Error { .. })).await;
    let Some(Event::Error { error, .. }) = seen.last() else {
        panic!();
    };
    assert!(
        error.message.contains("not controllable"),
        "{}",
        error.message
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn refused_connection_fails_and_disconnect_is_reported() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    let h = harness();
    let id = ServerId::next();
    h.commands
        .send(Command::Connect {
            server_id: id,
            http_base: url::Url::parse(&format!("http://{addr}")).unwrap(),
        })
        .unwrap();
    h.until(|e| {
        matches!(
            e,
            Event::TransportState {
                state: TransportState::Failed(_),
                ..
            }
        )
    })
    .await;
    h.commands
        .send(Command::Disconnect { server_id: id })
        .unwrap();
    h.until(|e| {
        matches!(
            e,
            Event::TransportState {
                state: TransportState::Disconnected,
                ..
            }
        )
    })
    .await;
}

/// Max 9 answers `/?HOST_INFO` with its whole tree and announces no WebSocket. The connection
/// still succeeds: `HOST_INFO` defaults, OSC on the HTTP port, the single WebSocket attempt on
/// the HTTP address fails silently, and no error is raised at any point.
#[tokio::test(flavor = "multi_thread")]
async fn max_ignoring_host_info_connects_without_error() {
    let tree = read_fixture("max-root.json");
    let server = SyntheticServer::start(
        "127.0.0.1:0",
        vec![
            ("/", Reply::json(tree.clone())),
            (
                "/?HOST_INFO",
                Reply::json(read_fixture("max-hostinfo.json")),
            ),
        ],
    )
    .await
    .unwrap();
    let h = harness();
    let id = ServerId::next();
    h.commands
        .send(Command::Connect {
            server_id: id,
            http_base: server.base_url(),
        })
        .unwrap();
    let seen = h
        .until(|e| {
            matches!(
                e,
                Event::TransportState {
                    state: TransportState::HttpOnly,
                    ..
                }
            )
        })
        .await;
    let (info, destination) = seen
        .iter()
        .find_map(|e| match e {
            Event::HostInfo {
                info, destination, ..
            } => Some((info.clone(), destination.clone().unwrap())),
            _ => None,
        })
        .unwrap();
    assert!(info.query_ignored);
    assert_eq!(
        (destination.port, destination.port_source),
        (server.addr.port(), PortSource::InferredFromHttp)
    );
    let loaded = seen
        .iter()
        .find_map(|e| match e {
            Event::TreeLoaded { report, .. } => Some(report.root.count()),
            _ => None,
        })
        .unwrap();
    assert_eq!(loaded, 36);
    // The WebSocket attempt on the HTTP address is refused by a plain HTTP server.
    let later = h.quiet(Duration::from_millis(1500)).await;
    assert!(
        seen.iter()
            .chain(&later)
            .all(|e| !matches!(e, Event::Error { .. } | Event::HostInfoUnavailable { .. })),
        "{seen:?} {later:?}"
    );
    assert!(
        later
            .iter()
            .all(|e| !matches!(e, Event::TransportState { .. })),
        "refresh only stays, silently: {later:?}"
    );
}

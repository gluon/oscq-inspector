//! The WebSocket part of the service against the synthetic server: listens, value frames,
//! structure commands, reconnection, and the fallback to refresh only.

#![allow(clippy::unwrap_used)] // test code, unwrap is allowed outside production paths (PRD 11)

mod common;

use std::time::Duration;

use common::{Harness, Push, Reply, SyntheticServer, harness};
use oscq_core::{AttributeReply, Command, Direction, Event, ServerId, TransportState};
use rosc::{OscBundle, OscMessage, OscPacket, OscTime, OscType};
use serde_json::json;

const TREE: &str = r#"{"FULL_PATH":"/","CONTENTS":{
    "a":{"FULL_PATH":"/a","CONTENTS":{
        "x":{"FULL_PATH":"/a/x","TYPE":"f","ACCESS":3,"VALUE":[0.1]},
        "y":{"FULL_PATH":"/a/y","TYPE":"f","ACCESS":3,"VALUE":[0.2]}}},
    "b":{"FULL_PATH":"/b","TYPE":"i","ACCESS":3,"VALUE":[1]}}}"#;
const SUBTREE_A: &str = r#"{"FULL_PATH":"/a","CONTENTS":{
    "x":{"FULL_PATH":"/a/x","TYPE":"f","ACCESS":3,"VALUE":[0.1]}}}"#;

fn routes(host_info: String) -> Vec<(&'static str, Reply)> {
    vec![
        ("/", Reply::json(TREE)),
        ("/a", Reply::json(SUBTREE_A)),
        ("/?HOST_INFO", Reply::json(host_info)),
    ]
}

/// HTTP and WebSocket on one port, `WS_IP` and `WS_PORT` not given: the default address.
async fn same_port_server() -> SyntheticServer {
    let host_info = r#"{"NAME":"syn","EXTENSIONS":{"LISTEN":true,"VALUE":true}}"#.to_owned();
    SyntheticServer::start_ws("127.0.0.1:0", routes(host_info))
        .await
        .unwrap()
}

fn is_state(e: &Event, want: fn(&TransportState) -> bool) -> bool {
    matches!(e, Event::TransportState { state, .. } if want(state))
}

fn live(s: &TransportState) -> bool {
    *s == TransportState::WebSocketLive
}

fn lost(s: &TransportState) -> bool {
    matches!(s, TransportState::WebSocketLost { .. })
}

async fn connect(h: &Harness, server: &SyntheticServer) -> (ServerId, Vec<Event>) {
    let id = ServerId::next();
    h.commands
        .send(Command::Connect {
            server_id: id,
            http_base: server.base_url(),
        })
        .unwrap();
    let seen = h.until(|e| is_state(e, live)).await;
    (id, seen)
}

fn listen(h: &Harness, id: ServerId, path: &str) {
    h.commands
        .send(Command::Listen {
            server_id: id,
            path: path.into(),
        })
        .unwrap();
}

fn ignore(h: &Harness, id: ServerId, path: &str) {
    h.commands
        .send(Command::Ignore {
            server_id: id,
            path: path.into(),
        })
        .unwrap();
}

fn set(paths: &[&str]) -> std::collections::HashSet<String> {
    paths.iter().map(|p| (*p).to_owned()).collect()
}

fn value_of(e: &Event, path: &str) -> Option<serde_json::Value> {
    match e {
        Event::ValueChanged {
            path: p,
            value: AttributeReply::Value(v),
            ..
        } if p == path => Some(v.clone()),
        _ => None,
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn same_port_websocket_goes_live_and_listens_follow_the_commands() {
    let server = same_port_server().await;
    let h = harness();
    let (id, seen) = connect(&h, &server).await;
    let order: Vec<bool> = seen
        .iter()
        .filter_map(|e| match e {
            Event::TransportState { state, .. } => Some(*state == TransportState::HttpOnly),
            _ => None,
        })
        .collect();
    assert_eq!(
        order,
        [false, true, false],
        "connecting, refresh only, live"
    );

    listen(&h, id, "/a/x");
    listen(&h, id, "/b");
    server
        .ws
        .wait("two listens", |w| w.listens() == set(&["/a/x", "/b"]))
        .await;

    server
        .ws
        .push(Push::Osc("/a/y".into(), vec![OscType::Float(0.9)]));
    server
        .ws
        .push(Push::Osc("/a/x".into(), vec![OscType::Float(0.75)]));
    let seen = h.until(|e| value_of(e, "/a/x").is_some()).await;
    assert_eq!(value_of(seen.last().unwrap(), "/a/x"), Some(json!([0.75])));
    assert!(
        seen.iter().all(|e| value_of(e, "/a/y").is_none()),
        "the server sends only what is listened to"
    );
    let logged = h.until(|e| matches!(e, Event::TrafficLogged(_))).await;
    let Some(Event::TrafficLogged(r)) = logged.last() else {
        panic!()
    };
    assert_eq!(
        (r.direction, r.path.as_str(), r.type_tags.as_str()),
        (Direction::In, "/a/x", "f")
    );

    ignore(&h, id, "/b");
    server
        .ws
        .wait("IGNORE /b", |w| w.listens() == set(&["/a/x"]))
        .await;
    let frames: Vec<serde_json::Value> = server
        .ws
        .received()
        .iter()
        .map(|t| serde_json::from_str(t).unwrap())
        .collect();
    assert_eq!(
        frames.last(),
        Some(&json!({"COMMAND":"IGNORE","DATA":"/b"}))
    );
    assert_eq!(
        frames.iter().filter(|f| f["COMMAND"] == "LISTEN").count(),
        2,
        "one LISTEN per path, never repeated"
    );
}

/// PRD 11: a WebSocket dropping mid session. Reconnected after the first backoff step, every
/// listen sent again, and the tree reloaded once for the changes the socket missed.
#[tokio::test(flavor = "multi_thread")]
async fn drop_mid_session_reconnects_and_listens_again() {
    let server = same_port_server().await;
    let h = harness();
    let (id, _) = connect(&h, &server).await;
    listen(&h, id, "/a/x");
    server
        .ws
        .wait("listen", |w| w.listens() == set(&["/a/x"]))
        .await;

    server.ws.push(Push::Drop);
    let seen = h.until(|e| is_state(e, live)).await;
    let retry = seen
        .iter()
        .find_map(|e| match e {
            Event::TransportState {
                state: TransportState::WebSocketLost { retry_in, .. },
                ..
            } => Some(*retry_in),
            _ => None,
        })
        .unwrap();
    assert!(
        (Duration::from_millis(400)..=Duration::from_millis(500)).contains(&retry),
        "{retry:?}"
    );
    server
        .ws
        .wait("listens re-issued", |w| {
            w.sessions() == 2 && w.listens() == set(&["/a/x"])
        })
        .await;
    h.until(|e| matches!(e, Event::SubtreeLoaded { path, .. } if path == "/"))
        .await;
    server
        .ws
        .push(Push::Osc("/a/x".into(), vec![OscType::Float(0.3)]));
    h.until(|e| value_of(e, "/a/x").is_some()).await;
}

/// PRD 11: a WebSocket advertised but refusing connections. Retried with a growing delay, no
/// error raised, and live as soon as the server accepts.
#[tokio::test(flavor = "multi_thread")]
async fn advertised_websocket_refusing_is_retried_until_it_accepts() {
    let ws_only = SyntheticServer::start("127.0.0.1:0", Vec::new())
        .await
        .unwrap();
    let host_info = format!(
        r#"{{"NAME":"syn","WS_IP":"127.0.0.1","WS_PORT":{}}}"#,
        ws_only.addr.port()
    );
    let server = SyntheticServer::start("127.0.0.1:0", routes(host_info))
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
    let mut delays = Vec::new();
    let mut seen = Vec::new();
    while delays.len() < 2 {
        seen.extend(h.until(|e| is_state(e, lost)).await);
        if let Some(Event::TransportState {
            state: TransportState::WebSocketLost { retry_in, .. },
            ..
        }) = seen.last()
        {
            delays.push(*retry_in);
        }
    }
    assert!(
        (Duration::from_millis(400)..=Duration::from_millis(500)).contains(&delays[0]),
        "{delays:?}"
    );
    assert!(
        (Duration::from_millis(800)..=Duration::from_millis(1000)).contains(&delays[1]),
        "{delays:?}"
    );
    assert_eq!(server.ws.sessions(), 0, "never tried on the HTTP port");

    listen(&h, id, "/b");
    ws_only.ws.set_accept(true);
    seen.extend(h.until(|e| is_state(e, live)).await);
    ws_only
        .ws
        .wait("listen sent once live", |w| w.listens() == set(&["/b"]))
        .await;
    assert!(
        seen.iter().all(|e| !matches!(e, Event::Error { .. })),
        "{seen:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn structure_commands_reload_and_move_listens() {
    let server = same_port_server().await;
    let h = harness();
    let (id, _) = connect(&h, &server).await;
    listen(&h, id, "/a/x");
    server
        .ws
        .wait("listen", |w| w.listens() == set(&["/a/x"]))
        .await;
    let text =
        |c: &str, d: serde_json::Value| Push::Text(json!({"COMMAND": c, "DATA": d}).to_string());
    let reloaded = |path: &'static str| move |e: &Event| matches!(e, Event::SubtreeLoaded { path: p, .. } if p == path);

    // A structure change reloads the subtree at the path, not a value.
    server.ws.push(text("PATH_CHANGED", json!("/a")));
    h.until(reloaded("/a")).await;

    // An addition reloads the parent.
    server.ws.push(text("PATH_ADDED", json!("/a/z")));
    let is_added = |e: &Event| matches!(e, Event::PathAdded { path, .. } if path == "/a/z");
    let seen = h.until(reloaded("/a")).await;
    if !seen.iter().any(is_added) {
        h.until(is_added).await;
    }

    // A rename moves the listen: after a reconnection it is sent for the new path.
    server
        .ws
        .push(text("PATH_RENAMED", json!({"OLD": "/a/x", "NEW": "/a/w"})));
    h.until(|e| matches!(e, Event::PathRenamed { old, new, .. } if old == "/a/x" && new == "/a/w"))
        .await;
    server.ws.push(Push::Drop);
    server
        .ws
        .wait("renamed listen", |w| {
            w.sessions() == 2 && w.listens() == set(&["/a/w"])
        })
        .await;

    // A removal drops the listens below it: after a reconnection nothing is listened to.
    server.ws.push(text("PATH_REMOVED", json!("/a")));
    h.until(|e| matches!(e, Event::PathRemoved { path, .. } if path == "/a"))
        .await;
    server.ws.push(Push::Drop);
    server.ws.wait("third session", |w| w.sessions() == 3).await;
    h.until(|e| is_state(e, live)).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(server.ws.listens().is_empty(), "{:?}", server.ws.listens());
}

/// No panic path from network input: bad frames are dropped and the session carries on.
#[tokio::test(flavor = "multi_thread")]
async fn hostile_frames_are_dropped_and_the_session_continues() {
    let server = same_port_server().await;
    let h = harness();
    let (id, _) = connect(&h, &server).await;
    listen(&h, id, "/a/x");
    server
        .ws
        .wait("listen", |w| w.listens() == set(&["/a/x"]))
        .await;
    let mut deep = OscPacket::Message(OscMessage {
        addr: "/a/x".into(),
        args: vec![OscType::Float(1.0)],
    });
    for _ in 0..20 {
        deep = OscPacket::Bundle(OscBundle {
            timetag: OscTime {
                seconds: 0,
                fractional: 1,
            },
            content: vec![deep],
        });
    }
    server
        .ws
        .push(Push::Raw(rosc::encoder::encode(&deep).unwrap()));
    server.ws.push(Push::Raw(b"\x00garbage".to_vec()));
    server.ws.push(Push::Text("not json".into()));
    server.ws.push(Push::Text(
        r#"{"COMMAND":"PATH_RENAMED","DATA":"/a"}"#.into(),
    ));
    server.ws.push(Push::Text(r#"{"COMMAND":"HELLO"}"#.into()));
    server
        .ws
        .push(Push::Osc("/a/x".into(), vec![OscType::Float(0.25)]));
    let seen = h.until(|e| value_of(e, "/a/x").is_some()).await;
    assert_eq!(value_of(seen.last().unwrap(), "/a/x"), Some(json!([0.25])));
    assert!(
        seen.iter()
            .all(|e| !is_state(e, lost) && !matches!(e, Event::Error { .. })),
        "{seen:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn disconnect_closes_the_websocket_for_good() {
    let server = same_port_server().await;
    let h = harness();
    let (id, _) = connect(&h, &server).await;
    h.commands
        .send(Command::Disconnect { server_id: id })
        .unwrap();
    h.until(|e| is_state(e, |s| *s == TransportState::Disconnected))
        .await;
    let later = h.quiet(Duration::from_millis(1500)).await;
    assert!(later.is_empty(), "{later:?}");
    assert_eq!(server.ws.sessions(), 1, "no reconnection");
}

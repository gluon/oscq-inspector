//! The `PEERS` and `CLIENT_NAME` extension (PRD 7.6) through the network service, against the
//! synthetic server: a server that declares it, one that declares nothing, one that declares it
//! and then fails, and replies no server should send.

#![allow(clippy::unwrap_used)] // test code, unwrap is allowed outside production paths (PRD 11)

mod common;

use std::sync::atomic::Ordering;
use std::time::Duration;

use common::{Harness, PeersMode, Push, Reply, SyntheticServer, harness_named, host_info_json};
use oscq_core::peers::{MAX_CLIENTS, MAX_PEERS_BODY};
use oscq_core::{Command, Event, PeersReading, ServerId, TransportState};
use serde_json::{Value, json};

const NAME: &str = "Inspector under test";

const DOC: &str = r#"{"FULL_PATH":"/","CONTENTS":{
    "a":{"FULL_PATH":"/a","TYPE":"f","ACCESS":3,"VALUE":[0.5]},
    "b":{"FULL_PATH":"/b","TYPE":"f","ACCESS":3,"VALUE":[0.25]}}}"#;

/// A server publishing [`DOC`], declaring the extension or not, with more routes if any.
async fn server(declares: bool, mode: PeersMode, more: Vec<(&str, Reply)>) -> SyntheticServer {
    let host_info = host_info_json("synthetic", 9, declares);
    let mut routes = vec![
        ("/", Reply::json(DOC)),
        ("/?HOST_INFO", Reply::json(host_info)),
    ];
    routes.extend(more);
    let s = SyntheticServer::start_ws("127.0.0.1:0", routes)
        .await
        .unwrap();
    s.ws.set_peers(mode);
    s
}

/// Connects the service to `s` and waits for its WebSocket.
async fn connect(h: &Harness, s: &SyntheticServer) -> ServerId {
    let id = ServerId::next();
    h.commands
        .send(Command::Connect {
            server_id: id,
            http_base: s.base_url(),
        })
        .unwrap();
    h.until(|e| {
        matches!(
            e,
            Event::TransportState {
                state: TransportState::WebSocketOpen,
                ..
            }
        )
    })
    .await;
    id
}

/// Asks for the peers of `id` and returns the answer.
async fn read_peers(h: &Harness, id: ServerId, quiet: bool) -> Result<PeersReading, String> {
    h.commands
        .send(Command::RefreshPeers {
            server_id: id,
            quiet,
        })
        .unwrap();
    let seen = h.until(|e| matches!(e, Event::Peers { .. })).await;
    match seen.into_iter().last() {
        Some(Event::Peers {
            server_id, result, ..
        }) => {
            assert_eq!(server_id, id);
            result
        }
        other => panic!("{other:?}"),
    }
}

fn commands(s: &SyntheticServer) -> Vec<(String, String)> {
    s.ws.received()
        .iter()
        .map(|t| {
            let v: Value = serde_json::from_str(t).unwrap();
            (
                v["COMMAND"].as_str().unwrap().to_owned(),
                v["DATA"].as_str().unwrap().to_owned(),
            )
        })
        .collect()
}

/// PRD 7.6: on a server that declares it, `CLIENT_NAME` is the first thing said on the socket,
/// and it is said again on the socket of each reconnection, before the listens are sent again.
#[tokio::test]
async fn client_name_is_sent_on_opening_the_socket_and_after_each_reconnection() {
    let s = server(true, PeersMode::Serve, Vec::new()).await;
    let h = harness_named(Some(NAME));
    let id = connect(&h, &s).await;
    h.commands
        .send(Command::Listen {
            server_id: id,
            path: "/a".into(),
        })
        .unwrap();
    s.ws.wait("the name, then the listen", |w| w.received().len() == 2)
        .await;
    let pair = |c: &str, d: &str| (c.to_owned(), d.to_owned());
    assert_eq!(
        commands(&s),
        [pair("CLIENT_NAME", NAME), pair("LISTEN", "/a")]
    );

    s.ws.push(Push::Drop);
    s.ws.wait("the socket opened again and said the same", |w| {
        w.sessions() == 2 && w.received().len() == 4
    })
    .await;
    assert_eq!(
        commands(&s),
        [
            pair("CLIENT_NAME", NAME),
            pair("LISTEN", "/a"),
            pair("CLIENT_NAME", NAME),
            pair("LISTEN", "/a"),
        ],
        "named again on the new socket, before its listens"
    );
}

/// PRD 7.6: a server that declares nothing is a normal server. It is never sent `CLIENT_NAME`
/// and never asked `?PEERS`, even by a command to read its peers, and even though this one
/// would answer: the answer to the command is the reason, with no request made. The same for
/// a server whose `HOST_INFO` could not be read, which has declared nothing either.
#[tokio::test]
async fn a_server_that_declares_nothing_is_never_named_to_nor_asked() {
    let silent = server(false, PeersMode::Serve, Vec::new()).await;
    let no_host_info = SyntheticServer::start_ws("127.0.0.1:0", vec![("/", Reply::json(DOC))])
        .await
        .unwrap();
    no_host_info.ws.set_peers(PeersMode::Serve);
    for s in [&silent, &no_host_info] {
        let h = harness_named(Some(NAME));
        let id = connect(&h, s).await;
        h.commands
            .send(Command::Listen {
                server_id: id,
                path: "/a".into(),
            })
            .unwrap();
        s.ws.wait("the listen", |w| !w.received().is_empty()).await;
        let reason = read_peers(&h, id, false).await.unwrap_err();
        assert!(reason.contains("does not declare PEERS"), "{reason}");
        // Nothing else is on its way: the listen was the last thing sent before the command.
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(
            commands(s),
            [("LISTEN".to_owned(), "/a".to_owned())],
            "no CLIENT_NAME"
        );
        let asked: Vec<String> =
            s.ws.requests()
                .into_iter()
                .filter(|t| t.contains("PEERS"))
                .collect();
        assert!(asked.is_empty(), "asked {asked:?}");
    }
    // A service with no name names itself to no one, declared or not.
    let s = server(true, PeersMode::Serve, Vec::new()).await;
    let h = harness_named(None);
    let id = connect(&h, &s).await;
    h.commands
        .send(Command::Listen {
            server_id: id,
            path: "/a".into(),
        })
        .unwrap();
    s.ws.wait("the listen", |w| !w.received().is_empty()).await;
    assert_eq!(commands(&s), [("LISTEN".to_owned(), "/a".to_owned())]);
}

/// PRD 7.6: `?PEERS` is asked on the root and nowhere else; the reply is read whole, every
/// client and every sender with the fields the server filled; and the client that is this
/// application's own socket is told from the others.
#[tokio::test]
async fn peers_are_read_on_the_root_and_this_client_is_found_among_them() {
    let s = server(true, PeersMode::Serve, Vec::new()).await;
    s.ws.set_other_peers(
        vec![
            json!({"IP": "10.0.0.9", "PORT": 5000, "NAME": "Max", "LISTEN": ["/a"], "PACKETS": 4}),
        ],
        vec![json!({"IP": "10.0.0.7", "PORT": 8000, "PACKETS": 2, "LAST_MS": 40})],
    );
    let h = harness_named(Some(NAME));
    let id = connect(&h, &s).await;
    h.commands
        .send(Command::Listen {
            server_id: id,
            path: "/b".into(),
        })
        .unwrap();
    s.ws.wait("the listen", |w| w.listens().contains("/b"))
        .await;

    let reading = read_peers(&h, id, false).await.unwrap();
    let asked: Vec<String> =
        s.ws.requests()
            .into_iter()
            .filter(|t| t.contains("PEERS"))
            .collect();
    assert_eq!(asked, ["/?PEERS"], "on the root only");
    assert_eq!(reading.this_client, Some(0), "its own socket, found");
    let [own, max] = &reading.peers.clients[..] else {
        panic!("{:?}", reading.peers.clients)
    };
    assert_eq!(own.name.as_deref(), Some(NAME));
    assert_eq!(own.listen, Some(vec!["/b".to_owned()]));
    assert_eq!(own.packets, Some(0));
    assert_eq!((own.osc_port, own.sender_port), (None, None), "not filled");
    assert_eq!(
        (max.ip.as_str(), max.port, max.name.as_deref(), max.packets),
        ("10.0.0.9", 5000, Some("Max"), Some(4))
    );
    assert_eq!(max.listen, Some(vec!["/a".to_owned()]));
    let [sender] = &reading.peers.senders[..] else {
        panic!("{:?}", reading.peers.senders)
    };
    assert_eq!(
        (
            sender.ip.as_str(),
            sender.port,
            sender.packets,
            sender.last_ms
        ),
        ("10.0.0.7", 8000, Some(2), Some(40))
    );

    // The socket is lost: this application is no longer among the clients, and says so.
    s.ws.set_accept(false);
    s.ws.push(Push::Drop);
    h.until(|e| {
        matches!(
            e,
            Event::TransportState {
                state: TransportState::WebSocketLost { .. },
                ..
            }
        )
    })
    .await;
    let reading = read_peers(&h, id, false).await.unwrap();
    assert_eq!(reading.this_client, None);
    assert_eq!(reading.peers.clients.len(), 1, "Max alone");
}

/// A reading asked quietly is queued without waking the UI; one asked aloud wakes it once.
#[tokio::test]
async fn a_quiet_reading_does_not_wake_the_ui() {
    let s = server(true, PeersMode::Serve, Vec::new()).await;
    let h = harness_named(Some(NAME));
    let id = connect(&h, &s).await;
    h.quiet(Duration::from_millis(200)).await;
    let before = h.notified.load(Ordering::SeqCst);
    read_peers(&h, id, true).await.unwrap();
    assert_eq!(
        h.notified.load(Ordering::SeqCst),
        before,
        "queued, the UI not woken"
    );
    read_peers(&h, id, false).await.unwrap();
    assert_eq!(h.notified.load(Ordering::SeqCst), before + 1);
}

/// PRD 9.10: a server that declares the extension and then fails on `?PEERS` is reported with
/// the failure and its reason. Law 5: a reply over a bound, or one that is not a reply, is a
/// failure too, with its reason, and the service goes on answering.
#[tokio::test]
async fn a_server_that_declares_then_fails_is_reported_with_the_reason() {
    let failing = server(true, PeersMode::Fail, Vec::new()).await;
    let h = harness_named(Some(NAME));
    let id = connect(&h, &failing).await;
    let reason = read_peers(&h, id, false).await.unwrap_err();
    assert!(reason.contains("answered HTTP 500"), "{reason}");
    assert!(reason.contains("/?PEERS"), "the request is named: {reason}");

    let entry = |i: usize| json!({"IP": "10.0.0.5", "PORT": 1 + i});
    let too_many: Vec<Value> = (0..=MAX_CLIENTS).map(entry).collect();
    let too_large = format!(
        r#"{{"CLIENTS":[],"SENDERS":[],"PAD":"{}"}}"#,
        "x".repeat(MAX_PEERS_BODY)
    );
    let deep = format!(
        r#"{{"CLIENTS":{}{}}}"#,
        "[".repeat(100_000),
        "]".repeat(100_000)
    );
    let cases: Vec<(Reply, &str)> = vec![
        (
            Reply::json(json!({ "CLIENTS": too_many }).to_string()),
            "CLIENTS holds 257 entries, more than the 256 this client reads",
        ),
        (Reply::json(too_large), "is larger than 4194304 bytes"),
        (Reply::json(deep), "nested deeper than"),
        (Reply::json("not json"), "is not JSON"),
        (Reply::json("{}"), "holds neither CLIENTS nor SENDERS"),
        (Reply::json(DOC), "holds neither CLIENTS nor SENDERS"),
        (Reply::NoContent, "answered 204 with no document"),
        (
            Reply::json(r#"{"CLIENTS":[{"IP":"10.0.0.5"}]}"#),
            "CLIENTS entry 0 is not an object with an IP and a PORT",
        ),
    ];
    for (reply, expected) in cases {
        let s = server(true, PeersMode::Serve, vec![("/?PEERS", reply)]).await;
        let id = connect(&h, &s).await;
        let reason = read_peers(&h, id, false).await.unwrap_err();
        assert!(reason.contains(expected), "{expected:?} not in {reason:?}");
    }
    // The same service, after all of them, still reads a server that answers.
    let good = server(true, PeersMode::Serve, Vec::new()).await;
    let id = connect(&h, &good).await;
    let reading = read_peers(&h, id, false).await.unwrap();
    assert_eq!(reading.this_client, Some(0));
}

/// The answer to a command for a server the service is not connected to is the reason.
#[tokio::test]
async fn peers_of_a_server_not_connected_are_not_read() {
    let h = harness_named(Some(NAME));
    let reason = read_peers(&h, ServerId::next(), false).await.unwrap_err();
    assert_eq!(reason, "not connected");
}

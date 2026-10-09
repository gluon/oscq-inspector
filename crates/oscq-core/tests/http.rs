//! The HTTP client and UDP sending against the synthetic server.

#![allow(clippy::unwrap_used)] // test code, unwrap is allowed outside production paths (PRD 11)

mod common;

use std::time::Duration;

use common::{OscReceiver, Reply, SyntheticServer, large_document, read_fixture};
use oscq_core::{
    AttributeReply, ClientConfig, ClientError, DocumentError, OscError, OscPath, OscQueryClient,
    OscUdpSender, PortSource, SendBlocked,
};
use rosc::{OscMessage, OscPacket, OscType};
use serde_json::json;

fn client(server: &SyntheticServer) -> OscQueryClient {
    OscQueryClient::new(server.base_url()).unwrap()
}

fn quick(server: &SyntheticServer) -> OscQueryClient {
    let config = ClientConfig {
        connect_timeout: Duration::from_millis(500),
        request_timeout: Duration::from_millis(500),
        max_body: 1024,
    };
    OscQueryClient::with_config(server.base_url(), config).unwrap()
}

/// The common real world layout: OSC receiver on a different port from the HTTP server.
#[tokio::test]
async fn osc_port_differs_from_http_port() {
    let target = OscReceiver::bind("127.0.0.1:0").await.unwrap();
    let host_info = format!(
        r#"{{"NAME":"x","OSC_PORT":{},"EXTENSIONS":{{"VALUE":true}}}}"#,
        target.port()
    );
    let tree = read_fixture("documents/basic_tree.json");
    let server = SyntheticServer::start(
        "127.0.0.1:0",
        vec![
            ("/", Reply::json(tree.clone())),
            ("/?HOST_INFO", Reply::json(host_info)),
        ],
    )
    .await
    .unwrap();
    let c = client(&server);

    let (info, diags) = c.fetch_host_info().await.unwrap();
    assert!(diags.is_empty());
    let dest = info.osc_destination(c.http_host(), c.http_port()).unwrap();
    assert_eq!(dest.port, target.port());
    assert_ne!(dest.port, c.http_port());
    assert_eq!(dest.port_source, PortSource::Advertised);

    let doc = c.fetch_tree("/").await.unwrap();
    assert_eq!(doc.raw.as_bytes(), tree.as_slice(), "raw body kept exactly");
    let bar = doc.report.root.find("/bar").unwrap();
    assert!(bar.effective_access(info.supports("VALUE")).writable());

    let mut sender = OscUdpSender::new();
    sender
        .send_message(
            &dest,
            bar.path.as_ref().unwrap(),
            bar.signature.as_ref().unwrap(),
            vec![OscType::Float(4.4), OscType::Double(60.0)],
        )
        .await
        .unwrap();
    assert_eq!(
        target.recv().await,
        OscPacket::Message(OscMessage {
            addr: "/bar".into(),
            args: vec![OscType::Int(4), OscType::Int(60)],
        })
    );
}

/// `OSC_PORT` absent: the HTTP port is used, and the packet really arrives there over UDP.
#[tokio::test]
async fn osc_port_absent_sends_to_http_port() {
    let server = SyntheticServer::start(
        "127.0.0.1:0",
        vec![(
            "/?HOST_INFO",
            Reply::json(read_fixture("host_info/no_osc_port.json")),
        )],
    )
    .await
    .unwrap();
    let target = OscReceiver::bind(&server.addr.to_string()).await.unwrap();
    let c = client(&server);
    let (info, _) = c.fetch_host_info().await.unwrap();
    let dest = info.osc_destination(c.http_host(), c.http_port()).unwrap();
    assert_eq!(
        (dest.port, dest.port_source),
        (c.http_port(), PortSource::InferredFromHttp)
    );

    let sig = oscq_core::TypeSignature::parse("f").unwrap();
    let path = OscPath::parse("/level").unwrap();
    OscUdpSender::new()
        .send_message(&dest, &path, &sig, vec![OscType::Int(1)])
        .await
        .unwrap();
    assert_eq!(
        target.recv().await,
        OscPacket::Message(OscMessage {
            addr: "/level".into(),
            args: vec![OscType::Float(1.0)]
        })
    );
}

#[tokio::test]
async fn tcp_transport_refuses_to_send() {
    let server = SyntheticServer::start(
        "127.0.0.1:0",
        vec![(
            "/?HOST_INFO",
            Reply::json(read_fixture("host_info/tcp.json")),
        )],
    )
    .await
    .unwrap();
    let c = client(&server);
    let (info, _) = c.fetch_host_info().await.unwrap();
    let dest = info.osc_destination(c.http_host(), c.http_port()).unwrap();
    let sig = oscq_core::TypeSignature::parse("f").unwrap();
    let err = OscUdpSender::new()
        .send_message(
            &dest,
            &OscPath::parse("/x").unwrap(),
            &sig,
            vec![OscType::Float(1.0)],
        )
        .await
        .unwrap_err();
    assert!(matches!(err, OscError::Blocked(SendBlocked::TcpTransport)));
}

#[tokio::test]
async fn attribute_queries_200_empty_and_204() {
    let server = SyntheticServer::start(
        "127.0.0.1:0",
        vec![
            ("/foo?VALUE", Reply::json(r#"{"VALUE":[0.5]}"#)),
            ("/baz?TYPE", Reply::json("{}")),
            (
                "/foo?CLIPMODE",
                Reply::json(r#"{"DESCRIPTION":"ignores the query"}"#),
            ),
            ("/silent?VALUE", Reply::NoContent),
            ("/missing?VALUE", Reply::Body(404, Vec::new())),
        ],
    )
    .await
    .unwrap();
    let c = client(&server);
    assert_eq!(
        c.fetch_attribute("/foo", "VALUE").await.unwrap(),
        AttributeReply::Value(json!([0.5]))
    );
    assert_eq!(
        c.fetch_attribute("/baz", "TYPE").await.unwrap(),
        AttributeReply::Unsupported
    );
    assert_eq!(
        c.fetch_attribute("/foo", "CLIPMODE").await.unwrap(),
        AttributeReply::Unsupported
    );
    assert_eq!(
        c.fetch_attribute("/silent", "VALUE").await.unwrap(),
        AttributeReply::NoContent
    );
    assert!(matches!(
        c.fetch_attribute("/missing", "VALUE").await,
        Err(ClientError::Status { status: 404, .. })
    ));
}

#[tokio::test]
async fn error_statuses() {
    let server = SyntheticServer::start(
        "127.0.0.1:0",
        vec![
            ("/bad", Reply::Body(400, Vec::new())),
            ("/boom", Reply::Body(500, b"{}".to_vec())),
            ("/empty", Reply::NoContent),
        ],
    )
    .await
    .unwrap();
    let c = client(&server);
    for (path, code) in [("/bad", 400), ("/boom", 500), ("/nowhere", 404)] {
        match c.fetch_tree(path).await {
            Err(ClientError::Status { status, .. }) => assert_eq!(status, code, "{path}"),
            other => panic!("{path}: {other:?}"),
        }
    }
    assert!(matches!(
        c.fetch_tree("/empty").await,
        Err(ClientError::NoDocument { .. })
    ));
}

#[tokio::test]
async fn host_info_missing() {
    let server = SyntheticServer::start(
        "127.0.0.1:0",
        vec![("/", Reply::json(r#"{"FULL_PATH":"/"}"#))],
    )
    .await
    .unwrap();
    let c = client(&server);
    assert!(c.fetch_tree("/").await.is_ok(), "browsable");
    assert!(matches!(
        c.fetch_host_info().await,
        Err(ClientError::Status { status: 404, .. })
    ));
}

#[tokio::test]
async fn connection_refused() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    let c = OscQueryClient::new(url::Url::parse(&format!("http://{addr}")).unwrap()).unwrap();
    assert!(matches!(
        c.fetch_tree("/").await,
        Err(ClientError::Connect { .. })
    ));
}

#[tokio::test]
async fn timeout() {
    let server = SyntheticServer::start("127.0.0.1:0", vec![("/", Reply::Hang)])
        .await
        .unwrap();
    assert!(matches!(
        quick(&server).fetch_tree("/").await,
        Err(ClientError::Timeout { .. })
    ));
}

#[tokio::test]
async fn body_not_json_or_not_an_address_space() {
    let server = SyntheticServer::start(
        "127.0.0.1:0",
        vec![
            ("/", Reply::json(read_fixture("documents/not_json.txt"))),
            (
                "/a",
                Reply::json(read_fixture("documents/not_an_address_space.json")),
            ),
            ("/?HOST_INFO", Reply::json("[]")),
        ],
    )
    .await
    .unwrap();
    let c = client(&server);
    assert!(matches!(
        c.fetch_tree("/").await,
        Err(ClientError::Document {
            source: DocumentError::NotJson(_),
            ..
        })
    ));
    assert!(matches!(
        c.fetch_tree("/a").await,
        Err(ClientError::Document {
            source: DocumentError::NotAnAddressSpace,
            ..
        })
    ));
    assert!(matches!(
        c.fetch_host_info().await,
        Err(ClientError::Document {
            source: DocumentError::NotAnObject,
            ..
        })
    ));
}

#[tokio::test]
async fn body_size_cap() {
    let big = large_document(100);
    assert!(big.len() > 1024);
    let server = SyntheticServer::start("127.0.0.1:0", vec![("/", Reply::json(big))])
        .await
        .unwrap();
    assert!(matches!(
        quick(&server).fetch_tree("/").await,
        Err(ClientError::BodyTooLarge { limit: 1024, .. })
    ));
}

#[tokio::test]
async fn five_thousand_node_document_over_http() {
    let server = SyntheticServer::start(
        "127.0.0.1:0",
        vec![("/", Reply::json(large_document(5000)))],
    )
    .await
    .unwrap();
    let doc = client(&server).fetch_tree("/").await.unwrap();
    assert_eq!(doc.report.root.count(), 5051);
}

#[tokio::test]
async fn subtree_and_encoded_paths() {
    let server = SyntheticServer::start(
        "127.0.0.1:0",
        vec![("/baz", Reply::json(r#"{"FULL_PATH":"/baz","CONTENTS":{}}"#))],
    )
    .await
    .unwrap();
    let doc = client(&server).fetch_tree("/baz").await.unwrap();
    assert_eq!(doc.report.root.full_path, "/baz");
}

/// IPv6 only server: HTTP over `[::1]` and OSC to an IPv6 receiver.
#[tokio::test]
async fn ipv6_only() {
    let target = OscReceiver::bind("[::1]:0").await.unwrap();
    let host_info = format!(r#"{{"OSC_IP":"::1","OSC_PORT":{}}}"#, target.port());
    let server = SyntheticServer::start(
        "[::1]:0",
        vec![
            ("/", Reply::json(read_fixture("documents/basic_tree.json"))),
            ("/?HOST_INFO", Reply::json(host_info)),
        ],
    )
    .await
    .unwrap();
    let c = client(&server);
    assert_eq!(c.http_host(), "::1");
    assert_eq!(c.fetch_tree("/").await.unwrap().report.root.count(), 5);
    let (info, _) = c.fetch_host_info().await.unwrap();
    let dest = info.osc_destination(c.http_host(), c.http_port()).unwrap();
    let sig = oscq_core::TypeSignature::parse("s").unwrap();
    OscUdpSender::new()
        .send_message(
            &dest,
            &OscPath::parse("/baz/qux").unwrap(),
            &sig,
            vec![OscType::String("full".into())],
        )
        .await
        .unwrap();
    assert_eq!(
        target.recv().await,
        OscPacket::Message(OscMessage {
            addr: "/baz/qux".into(),
            args: vec![OscType::String("full".into())]
        })
    );
}

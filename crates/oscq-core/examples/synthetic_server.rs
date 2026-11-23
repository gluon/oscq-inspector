//! The synthetic OSCQuery server of the test suite, as a program: `large_document` of 5000
//! methods by default, HTTP and WebSocket on one port, OSC received on a UDP port it advertises.
//! With `--stream HZ` it changes the value of every listened method HZ times a second, each
//! method on its own phase so that no two show the same value, which the app shows without user
//! action when its WebSocket is live. `?VALUE` answers the value last pushed.
//!
//! The `PEERS` and `CLIENT_NAME` extension (PRD 7.6), with `--peers MODE`: `serve` declares it
//! and answers `?PEERS` with the clients connected; `fail` declares it and answers 500; `off`,
//! the default, declares nothing and answers 404, as a server that knows nothing of it. `--name
//! NAME` is the name its `HOST_INFO` gives.
//!
//! `--doc FILE` publishes the document of FILE in place of `large_document`: each container and
//! each method answers its own subtree request, and each method holding a value its `?VALUE`.
//! `--stream` then changes the float methods alone, each within its `RANGE`. `demo-space.json`,
//! beside this file, is the address space of a small audiovisual rig, for the documentation
//! images. `--echo` sends every OSC message received back over the WebSocket to the clients
//! listening to its path, as a server confirming a change.
//!
//! `--host-info FILE` serves the JSON of FILE as the whole `HOST_INFO`, byte for byte, in place of
//! the one this server would write: the way to put a `HOST_INFO` no server should send, an empty
//! `WS_IP` for instance, in front of a client. `--name` then does not reach it, and what it
//! declares, `EXTENSIONS` included, is the file's. `host-info-empty-ws-ip.json`, beside this
//! file, is that empty `WS_IP`, and `one-method.json` a document of a single method.
//!
//! ```text
//! cargo run -p oscq-core --example synthetic_server -- --port 5678 --nodes 5000 --stream 2
//! cargo run -p oscq-core --example synthetic_server -- --port 5679 --peers serve --name Alpha
//! cargo run -p oscq-core --example synthetic_server -- --port 5680 --peers serve --echo \
//!   --doc crates/oscq-core/examples/demo-space.json --name "AV rig" --stream 4
//! cargo run -p oscq-core --example synthetic_server -- --port 5690 \
//!   --doc crates/oscq-core/examples/one-method.json \
//!   --host-info crates/oscq-core/examples/host-info-empty-ws-ip.json
//! ```

#![allow(clippy::unwrap_used)] // shares the test helpers, which unwrap

#[path = "../tests/common/mod.rs"]
mod common;

use std::time::Duration;

use common::{
    OscReceiver, PeersMode, Push, Reply, SyntheticServer, borrowed, host_info_json, large_routes,
};
use rosc::{OscPacket, OscType};

fn arg(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1).cloned())
}

/// A float method of a `--doc` document: its path and the bounds of its `RANGE`, 0 and 1 when
/// it declares none.
struct Float {
    path: String,
    min: f32,
    max: f32,
}

/// A route and its reply, as [`large_routes`] gives them.
type Routes = Vec<(String, Reply)>;

/// The routes of the document in `file`: every node at its `FULL_PATH`, and the `?VALUE` of
/// every method holding a value. With them, the float methods, the ones `--stream` changes.
fn doc_routes(file: &str) -> anyhow::Result<(Routes, Vec<Float>)> {
    let doc: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(file)?)?;
    let (mut routes, mut floats) = (Vec::new(), Vec::new());
    let mut stack = vec![&doc];
    while let Some(node) = stack.pop() {
        let Some(path) = node["FULL_PATH"].as_str() else {
            anyhow::bail!("{file}: a node without FULL_PATH");
        };
        routes.push((path.to_owned(), Reply::json(node.to_string())));
        if let Some(value) = node.get("VALUE") {
            let body = serde_json::json!({ "VALUE": value }).to_string();
            routes.push((format!("{path}?VALUE"), Reply::json(body)));
        }
        if node["TYPE"] == "f" {
            let range = &node["RANGE"][0];
            floats.push(Float {
                path: path.to_owned(),
                min: range["MIN"].as_f64().unwrap_or(0.0) as f32,
                max: range["MAX"].as_f64().unwrap_or(1.0) as f32,
            });
        }
        if let Some(children) = node.get("CONTENTS").and_then(|c| c.as_object()) {
            stack.extend(children.values());
        }
    }
    Ok((routes, floats))
}

/// The messages of a packet, those of its bundles included, in order.
fn messages(packet: OscPacket, out: &mut Vec<rosc::OscMessage>) {
    match packet {
        OscPacket::Message(m) => out.push(m),
        OscPacket::Bundle(b) => b.content.into_iter().for_each(|p| messages(p, out)),
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let port: u16 = arg(&args, "--port").map_or(Ok(0), |p| p.parse())?;
    let nodes: usize = arg(&args, "--nodes").map_or(Ok(5000), |n| n.parse())?;
    let stream: f64 = arg(&args, "--stream").map_or(Ok(0.0), |n| n.parse())?;
    let peers = match arg(&args, "--peers").as_deref() {
        None | Some("off") => PeersMode::Off,
        Some("serve") => PeersMode::Serve,
        Some("fail") => PeersMode::Fail,
        Some(other) => anyhow::bail!("--peers {other}: serve, fail or off"),
    };
    let name = arg(&args, "--name").unwrap_or_else(|| format!("synthetic {nodes}"));
    let doc = arg(&args, "--doc");
    let echo = args.iter().any(|a| a == "--echo");
    let host_info = arg(&args, "--host-info");

    let osc = OscReceiver::bind("127.0.0.1:0").await?;
    let (mut routes, floats) = match &doc {
        Some(file) => doc_routes(file)?,
        None => (large_routes(nodes, osc.port()), Vec::new()),
    };
    let methods = match &doc {
        Some(_) => routes.iter().filter(|(k, _)| k.ends_with("?VALUE")).count(),
        None => nodes,
    };
    // Served as it is, so that a `HOST_INFO` this server would never write itself can be put in
    // front of a client: an attribute present and empty, for one, which counts as absent.
    let host_info_body = match &host_info {
        Some(file) => {
            std::fs::read(file).map_err(|e| anyhow::anyhow!("--host-info {file}: {e}"))?
        }
        None => host_info_json(&name, osc.port(), peers != PeersMode::Off).into_bytes(),
    };
    routes.push(("/?HOST_INFO".to_owned(), Reply::json(host_info_body)));
    let server = SyntheticServer::start_ws(&format!("127.0.0.1:{port}"), borrowed(&routes)).await?;
    server.ws.set_peers(peers);
    println!(
        "http://{}  {name}  {methods} methods holding a value  OSC on udp 127.0.0.1:{}  peers {peers:?}",
        server.addr,
        osc.port()
    );
    if let Some(file) = &host_info {
        println!("HOST_INFO from {file}, served as it is: --name does not reach it");
    }

    let received = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counter = std::sync::Arc::clone(&received);
    let ws = std::sync::Arc::clone(&server.ws);
    tokio::spawn(async move {
        let mut buf = vec![0u8; 65536];
        while let Ok(n) = osc.socket.recv(&mut buf).await {
            counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            if !echo {
                continue;
            }
            let Ok((_, packet)) = rosc::decoder::decode_udp(&buf[..n]) else {
                continue;
            };
            let mut received = Vec::new();
            messages(packet, &mut received);
            let listens = ws.listens();
            for m in received.into_iter().filter(|m| listens.contains(&m.addr)) {
                ws.push(Push::Osc(m.addr, m.args));
            }
        }
    });

    let tick = if stream > 0.0 {
        Duration::from_secs_f64(1.0 / stream)
    } else {
        Duration::from_secs(1)
    };
    let mut last = (usize::MAX, usize::MAX);
    let mut phase = 0.0f32;
    loop {
        tokio::time::sleep(tick).await;
        let listens = server.ws.listens();
        if stream > 0.0 {
            phase += 0.1;
            for p in &listens {
                let (min, max) = match floats.iter().find(|f| &f.path == p) {
                    Some(f) => (f.min, f.max),
                    None if doc.is_none() => (0.0, 1.0),
                    None => continue,
                };
                // A phase per path: values that differ from method to method.
                let offset = p.bytes().map(f32::from).sum::<f32>() * 0.37;
                let v = 0.5 + 0.5 * (phase + offset).sin();
                server.ws.push(Push::Osc(
                    p.clone(),
                    vec![OscType::Float(min + v * (max - min))],
                ));
            }
        }
        let now = (
            listens.len(),
            received.load(std::sync::atomic::Ordering::Relaxed),
        );
        if now != last {
            println!("listens {}  OSC packets received {}", now.0, now.1);
            last = now;
        }
    }
}

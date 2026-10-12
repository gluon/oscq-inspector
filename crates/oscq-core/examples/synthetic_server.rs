//! The synthetic OSCQuery server of the test suite, as a program: `large_document` of 5000
//! methods by default, HTTP and WebSocket on one port, OSC received on a UDP port it advertises.
//! With `--stream HZ` it changes the value of every listened method HZ times a second, each
//! method on its own phase so that no two show the same value, which the app shows without user
//! action when its WebSocket is live. `?VALUE` answers the value last pushed.
//!
//! ```text
//! cargo run -p oscq-core --example synthetic_server -- --port 5678 --nodes 5000 --stream 2
//! ```

#![allow(clippy::unwrap_used)] // shares the test helpers, which unwrap

#[path = "../tests/common/mod.rs"]
mod common;

use std::time::Duration;

use common::{OscReceiver, Push, SyntheticServer, borrowed, large_routes};
use rosc::OscType;

fn arg(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1).cloned())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let port: u16 = arg(&args, "--port").map_or(Ok(0), |p| p.parse())?;
    let nodes: usize = arg(&args, "--nodes").map_or(Ok(5000), |n| n.parse())?;
    let stream: f64 = arg(&args, "--stream").map_or(Ok(0.0), |n| n.parse())?;

    let osc = OscReceiver::bind("127.0.0.1:0").await?;
    let routes = large_routes(nodes, osc.port());
    let server = SyntheticServer::start_ws(&format!("127.0.0.1:{port}"), borrowed(&routes)).await?;
    println!(
        "http://{}  {nodes} methods  OSC on udp 127.0.0.1:{}",
        server.addr,
        osc.port()
    );

    let received = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counter = std::sync::Arc::clone(&received);
    tokio::spawn(async move {
        let mut buf = vec![0u8; 65536];
        while osc.socket.recv(&mut buf).await.is_ok() {
            counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
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
                // A phase per path: values that differ from method to method.
                let offset = p.bytes().map(f32::from).sum::<f32>() * 0.37;
                let v = 0.5 + 0.5 * (phase + offset).sin();
                server
                    .ws
                    .push(Push::Osc(p.clone(), vec![OscType::Float(v)]));
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

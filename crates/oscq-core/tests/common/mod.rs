//! Synthetic OSCQuery server and OSC receiver used by the integration tests.

#![allow(dead_code)]

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use rosc::OscPacket;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream, UdpSocket};
use url::Url;

/// Path to a file under the workspace `tests/fixtures` directory.
pub fn fixture(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .join(rel)
}

pub fn read_fixture(rel: &str) -> Vec<u8> {
    std::fs::read(fixture(rel)).unwrap_or_else(|e| panic!("fixture {rel}: {e}"))
}

#[derive(Clone)]
pub enum Reply {
    /// Status and body, served as `application/json`.
    Body(u16, Vec<u8>),
    /// 204, no body.
    NoContent,
    /// Accept the request and never answer.
    Hang,
}

impl Reply {
    pub fn json(body: impl Into<Vec<u8>>) -> Self {
        Reply::Body(200, body.into())
    }
}

/// A minimal HTTP/1.1 server answering GET requests from a fixed route table keyed by request
/// target, for example `/`, `/?HOST_INFO`, `/foo?VALUE`. Unknown targets answer 404.
pub struct SyntheticServer {
    pub addr: SocketAddr,
    task: tokio::task::JoinHandle<()>,
}

impl SyntheticServer {
    pub async fn start(bind: &str, routes: Vec<(&str, Reply)>) -> std::io::Result<Self> {
        let listener = TcpListener::bind(bind).await?;
        let addr = listener.local_addr()?;
        let routes: Arc<HashMap<String, Reply>> =
            Arc::new(routes.into_iter().map(|(k, v)| (k.to_owned(), v)).collect());
        let task = tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let routes = Arc::clone(&routes);
                tokio::spawn(async move {
                    let _ = serve(stream, &routes).await;
                });
            }
        });
        Ok(Self { addr, task })
    }

    pub fn base_url(&self) -> Url {
        Url::parse(&format!("http://{}", self.addr)).unwrap()
    }
}

impl Drop for SyntheticServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn serve(mut stream: TcpStream, routes: &HashMap<String, Reply>) -> std::io::Result<()> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 1024];
    while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
        let n = stream.read(&mut chunk).await?;
        if n == 0 || buf.len() > 16 * 1024 {
            return Ok(());
        }
        buf.extend_from_slice(&chunk[..n]);
    }
    let head = String::from_utf8_lossy(&buf);
    let target = head
        .lines()
        .next()
        .and_then(|l| l.split(' ').nth(1))
        .unwrap_or("")
        .to_owned();
    let reply = routes
        .get(&target)
        .cloned()
        .unwrap_or(Reply::Body(404, Vec::new()));
    let response = match reply {
        Reply::Hang => {
            tokio::time::sleep(Duration::from_secs(60)).await;
            return Ok(());
        }
        Reply::NoContent => b"HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n".to_vec(),
        Reply::Body(status, body) => {
            let mut r = format!(
                "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            )
            .into_bytes();
            r.extend_from_slice(&body);
            r
        }
    };
    stream.write_all(&response).await?;
    stream.shutdown().await
}

/// A UDP socket standing for the OSC receiver behind an OSCQuery server.
pub struct OscReceiver {
    pub socket: UdpSocket,
}

impl OscReceiver {
    pub async fn bind(addr: &str) -> std::io::Result<Self> {
        Ok(Self {
            socket: UdpSocket::bind(addr).await?,
        })
    }

    pub fn port(&self) -> u16 {
        self.socket.local_addr().unwrap().port()
    }

    /// Next OSC packet, decoded, or a panic after two seconds.
    pub async fn recv(&self) -> OscPacket {
        let mut buf = vec![0u8; 65536];
        let n = tokio::time::timeout(Duration::from_secs(2), self.socket.recv(&mut buf))
            .await
            .expect("no OSC packet within 2 s")
            .unwrap();
        rosc::decoder::decode_udp(&buf[..n]).unwrap().1
    }
}

/// A document of `n` methods spread over containers of 100 children, as a JSON string.
pub fn large_document(n: usize) -> String {
    let mut groups = Vec::new();
    for g in 0..n.div_ceil(100) {
        let mut children = Vec::new();
        for i in (g * 100)..((g + 1) * 100).min(n) {
            children.push(format!(
                r#""p{i}":{{"FULL_PATH":"/g{g}/p{i}","TYPE":"f","ACCESS":3,"VALUE":[0.5],"RANGE":[{{"MIN":0,"MAX":1}}]}}"#
            ));
        }
        groups.push(format!(
            r#""g{g}":{{"FULL_PATH":"/g{g}","CONTENTS":{{{}}}}}"#,
            children.join(",")
        ));
    }
    format!(r#"{{"FULL_PATH":"/","CONTENTS":{{{}}}}}"#, groups.join(","))
}

/// A chain of `depth` nested containers below the root, as a JSON string.
pub fn deep_document(depth: usize) -> String {
    let mut path = String::new();
    let mut open = String::from(r#"{"FULL_PATH":"/","CONTENTS":{"#);
    let mut close = String::from("}}");
    for i in 0..depth {
        path.push_str(&format!("/n{i}"));
        open.push_str(&format!(r#""n{i}":{{"FULL_PATH":"{path}","CONTENTS":{{"#));
        close.push_str("}}");
    }
    open + &close
}

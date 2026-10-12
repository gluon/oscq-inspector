//! Synthetic OSCQuery server and OSC receiver used by the integration tests.

#![allow(dead_code)]

use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use oscq_core::{Command, Event, service};
use rosc::{OscMessage, OscPacket, OscType};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream, UdpSocket};
use tokio::sync::broadcast;
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::handshake::derive_accept_key;
use tokio_tungstenite::tungstenite::protocol::Role;
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
/// target, for example `/`, `/?HOST_INFO`, `/foo?VALUE`. Unknown targets answer 404. When its
/// WebSocket is accepting, an upgrade request on any target opens a session driven by
/// [`WsControl`]; otherwise the upgrade is answered like a plain GET.
pub struct SyntheticServer {
    pub addr: SocketAddr,
    pub ws: Arc<WsControl>,
    task: tokio::task::JoinHandle<()>,
}

/// Something for every open WebSocket session to do.
#[derive(Debug, Clone)]
pub enum Push {
    /// A text frame, as is.
    Text(String),
    /// An OSC message, sent only by the sessions listening to its path, as a real server does.
    Osc(String, Vec<OscType>),
    /// A binary frame, as is, to every session.
    Raw(Vec<u8>),
    /// Drop the TCP connection without a close frame.
    Drop,
}

/// The WebSocket side of a [`SyntheticServer`], shared with the test.
pub struct WsControl {
    accept: AtomicBool,
    push: broadcast::Sender<Push>,
    /// Text frames received from clients, in order.
    received: Mutex<Vec<String>>,
    /// Paths listened to on the most recent session.
    listens: Mutex<HashSet<String>>,
    /// Sessions opened so far.
    sessions: AtomicUsize,
    /// The last value pushed for each path, which `?VALUE` answers.
    values: Mutex<HashMap<String, Vec<OscType>>>,
}

impl WsControl {
    fn new(accept: bool) -> Self {
        Self {
            accept: AtomicBool::new(accept),
            push: broadcast::channel(1024).0,
            received: Mutex::new(Vec::new()),
            listens: Mutex::new(HashSet::new()),
            sessions: AtomicUsize::new(0),
            values: Mutex::new(HashMap::new()),
        }
    }

    /// Whether upgrade requests are accepted from now on.
    pub fn set_accept(&self, accept: bool) {
        self.accept.store(accept, Ordering::SeqCst);
    }

    pub fn push(&self, p: Push) {
        if let Push::Osc(path, args) = &p {
            self.values
                .lock()
                .unwrap()
                .insert(path.clone(), args.clone());
        }
        let _ = self.push.send(p);
    }

    pub fn received(&self) -> Vec<String> {
        self.received.lock().unwrap().clone()
    }

    pub fn listens(&self) -> HashSet<String> {
        self.listens.lock().unwrap().clone()
    }

    pub fn sessions(&self) -> usize {
        self.sessions.load(Ordering::SeqCst)
    }

    /// Waits until `pred` holds, or panics after five seconds.
    pub async fn wait(&self, what: &str, pred: impl Fn(&Self) -> bool) {
        let end = std::time::Instant::now() + Duration::from_secs(5);
        while !pred(self) {
            assert!(
                std::time::Instant::now() < end,
                "timed out waiting for {what}: received {:?}, listens {:?}",
                self.received(),
                self.listens()
            );
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }
}

impl SyntheticServer {
    /// HTTP only: an upgrade request gets the route's plain answer.
    pub async fn start(bind: &str, routes: Vec<(&str, Reply)>) -> std::io::Result<Self> {
        Self::start_with(bind, routes, false).await
    }

    /// HTTP and a WebSocket on the same port, accepting from the start.
    pub async fn start_ws(bind: &str, routes: Vec<(&str, Reply)>) -> std::io::Result<Self> {
        Self::start_with(bind, routes, true).await
    }

    async fn start_with(
        bind: &str,
        routes: Vec<(&str, Reply)>,
        accept_ws: bool,
    ) -> std::io::Result<Self> {
        let listener = TcpListener::bind(bind).await?;
        let addr = listener.local_addr()?;
        let routes: Arc<HashMap<String, Reply>> =
            Arc::new(routes.into_iter().map(|(k, v)| (k.to_owned(), v)).collect());
        let ws = Arc::new(WsControl::new(accept_ws));
        let control = Arc::clone(&ws);
        let task = tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let routes = Arc::clone(&routes);
                let control = Arc::clone(&control);
                tokio::spawn(async move {
                    let _ = serve(stream, &routes, &control).await;
                });
            }
        });
        Ok(Self { addr, ws, task })
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

async fn serve(
    mut stream: TcpStream,
    routes: &HashMap<String, Reply>,
    ws: &WsControl,
) -> std::io::Result<()> {
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
    let header = |name: &str| {
        head.lines().find_map(|l| {
            let (k, v) = l.split_once(':')?;
            k.trim()
                .eq_ignore_ascii_case(name)
                .then(|| v.trim().to_owned())
        })
    };
    let upgrade = header("upgrade").is_some_and(|v| v.eq_ignore_ascii_case("websocket"));
    if upgrade && ws.accept.load(Ordering::SeqCst) {
        let key = header("sec-websocket-key").unwrap_or_default();
        let pushes = ws.push.subscribe();
        let response = format!(
            "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {}\r\n\r\n",
            derive_accept_key(key.as_bytes())
        );
        stream.write_all(response.as_bytes()).await?;
        let socket = WebSocketStream::from_raw_socket(stream, Role::Server, None).await;
        ws.sessions.fetch_add(1, Ordering::SeqCst);
        ws_session(socket, ws, pushes).await;
        return Ok(());
    }
    let target = head
        .lines()
        .next()
        .and_then(|l| l.split(' ').nth(1))
        .unwrap_or("")
        .to_owned();
    // A value pushed over the WebSocket is the server's current value: `?VALUE` answers it.
    let pushed = target
        .strip_suffix("?VALUE")
        .and_then(|p| ws.values.lock().unwrap().get(p).cloned())
        .map(|args| {
            let body = serde_json::json!({ "VALUE": oscq_core::ws::args_to_json(&args) });
            Reply::json(body.to_string())
        });
    let reply = pushed
        .or_else(|| routes.get(&target).cloned())
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

async fn ws_session(
    socket: WebSocketStream<TcpStream>,
    ws: &WsControl,
    mut pushes: broadcast::Receiver<Push>,
) {
    let (mut sink, mut incoming) = socket.split();
    let mut listens = HashSet::new();
    ws.listens.lock().unwrap().clear();
    loop {
        tokio::select! {
            m = incoming.next() => match m {
                Some(Ok(Message::Text(t))) => {
                    let v: serde_json::Value =
                        serde_json::from_str(t.as_str()).unwrap_or_default();
                    let path = v["DATA"].as_str().unwrap_or_default().to_owned();
                    match v["COMMAND"].as_str() {
                        Some("LISTEN") => {
                            listens.insert(path);
                        }
                        Some("IGNORE") => {
                            listens.remove(&path);
                        }
                        _ => {}
                    }
                    ws.received.lock().unwrap().push(t.as_str().to_owned());
                    *ws.listens.lock().unwrap() = listens.clone();
                }
                Some(Ok(_)) => {}
                Some(Err(_)) | None => return,
            },
            p = pushes.recv() => {
                let frame = match p {
                    Ok(Push::Text(t)) => Message::text(t),
                    Ok(Push::Osc(addr, args)) => {
                        if !listens.contains(&addr) {
                            continue;
                        }
                        let packet = OscPacket::Message(OscMessage { addr, args });
                        Message::binary(rosc::encoder::encode(&packet).unwrap())
                    }
                    Ok(Push::Raw(bytes)) => Message::binary(bytes),
                    Ok(Push::Drop) | Err(broadcast::error::RecvError::Closed) => return,
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                };
                if sink.send(frame).await.is_err() {
                    return;
                }
            }
        }
    }
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
    let groups: Vec<String> = (0..n.div_ceil(100))
        .map(|g| format!(r#""g{g}":{}"#, large_group(g, n)))
        .collect();
    format!(r#"{{"FULL_PATH":"/","CONTENTS":{{{}}}}}"#, groups.join(","))
}

/// The container `/g{g}` of [`large_document`], as its own subtree request returns it.
pub fn large_group(g: usize, n: usize) -> String {
    let children: Vec<String> = ((g * 100)..((g + 1) * 100).min(n))
        .map(|i| {
            format!(
                r#""p{i}":{{"FULL_PATH":"/g{g}/p{i}","TYPE":"f","ACCESS":3,"VALUE":[0.5],"RANGE":[{{"MIN":0,"MAX":1}}]}}"#
            )
        })
        .collect();
    format!(
        r#"{{"FULL_PATH":"/g{g}","CONTENTS":{{{}}}}}"#,
        children.join(",")
    )
}

/// The routes of a server publishing [`large_document`]: the root, each container, the `?VALUE`
/// of each method, and a `HOST_INFO` sending OSC to `osc_port`.
pub fn large_routes(n: usize, osc_port: u16) -> Vec<(String, Reply)> {
    let mut routes = vec![
        ("/".to_owned(), Reply::json(large_document(n))),
        (
            "/?HOST_INFO".to_owned(),
            Reply::json(format!(
                r#"{{"NAME":"synthetic {n}","OSC_PORT":{osc_port},"EXTENSIONS":{{"VALUE":true,"LISTEN":true}}}}"#
            )),
        ),
    ];
    routes.extend((0..n.div_ceil(100)).map(|g| (format!("/g{g}"), Reply::json(large_group(g, n)))));
    routes.extend((0..n).map(|i| {
        (
            format!("/g{}/p{i}?VALUE", i / 100),
            Reply::json(r#"{"VALUE":[0.5]}"#),
        )
    }));
    routes
}

/// Borrowed routes, the form [`SyntheticServer::start`] takes.
pub fn borrowed(routes: &[(String, Reply)]) -> Vec<(&str, Reply)> {
    routes
        .iter()
        .map(|(k, v)| (k.as_str(), v.clone()))
        .collect()
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

/// The network service on the current runtime, with its events and a count of notify calls.
pub struct Harness {
    pub commands: flume::Sender<Command>,
    pub events: flume::Receiver<Event>,
    pub notified: Arc<AtomicUsize>,
}

pub fn harness() -> Harness {
    let (tx, rx) = flume::unbounded();
    let notified = Arc::new(AtomicUsize::new(0));
    let n = Arc::clone(&notified);
    let commands = service::spawn(
        &tokio::runtime::Handle::current(),
        tx,
        Arc::new(move || {
            n.fetch_add(1, Ordering::SeqCst);
        }),
    );
    Harness {
        commands,
        events: rx,
        notified,
    }
}

impl Harness {
    pub async fn until(&self, mut pred: impl FnMut(&Event) -> bool) -> Vec<Event> {
        let mut seen = Vec::new();
        loop {
            let e = tokio::time::timeout(Duration::from_secs(5), self.events.recv_async())
                .await
                .unwrap_or_else(|_| panic!("timed out, seen: {seen:?}"))
                .unwrap();
            let done = pred(&e);
            seen.push(e);
            if done {
                return seen;
            }
        }
    }

    /// Every event that arrives within `window`.
    pub async fn quiet(&self, window: Duration) -> Vec<Event> {
        let end = tokio::time::Instant::now() + window;
        let mut seen = Vec::new();
        while let Ok(Ok(e)) = tokio::time::timeout_at(end, self.events.recv_async()).await {
            seen.push(e);
        }
        seen
    }
}

//! The network half of the application: commands in, events out, all I/O on a `tokio` runtime.
//!
//! The UI sends [`Command`]s on a `flume` channel and drains [`Event`]s from another. After each
//! event the service calls a notify callback, which the UI uses to request a repaint. Nothing here
//! knows about the UI toolkit.

use std::collections::{HashMap, HashSet};
use std::hash::BuildHasher;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use rosc::OscType;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use url::Url;

use crate::client::{AttributeReply, OscQueryClient};
use crate::document::ParseReport;
use crate::host_info::{HostInfo, OscDestination, SendBlocked};
use crate::osc::{OscUdpSender, coerce_args, encode_message};
use crate::path::OscPath;
use crate::server::{DiscoveredServer, ServerId};
use crate::types::TypeSignature;
use crate::ws::{self, ServerCommand};

/// UI to network.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// Fetch `HOST_INFO`, then the whole tree.
    Connect {
        server_id: ServerId,
        http_base: Url,
    },
    Disconnect {
        server_id: ServerId,
    },
    /// Fetch the subtree at a full path, its values included.
    RefreshTree {
        server_id: ServerId,
        from_path: String,
    },
    /// `?VALUE` on one path.
    RefreshValue {
        server_id: ServerId,
        path: String,
    },
    /// Coerce `args` to `signature`, send over UDP to the destination from `HOST_INFO`, log it,
    /// then refresh the value if `refresh_after`.
    SendOsc {
        server_id: ServerId,
        path: OscPath,
        signature: TypeSignature,
        args: Vec<OscType>,
        refresh_after: bool,
    },
    /// Ask for value updates of one method over the WebSocket. Kept across reconnections.
    Listen {
        server_id: ServerId,
        path: String,
    },
    /// Stop the value updates of one method.
    Ignore {
        server_id: ServerId,
        path: String,
    },
    Shutdown,
}

/// What a user is shown when something fails: the failing path, when there is one, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserFacingError {
    pub path: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransportState {
    Connecting,
    /// Tree loaded over HTTP, no WebSocket: values on refresh only.
    HttpOnly,
    /// WebSocket open: listened values are pushed by the server.
    WebSocketLive,
    /// The WebSocket failed or dropped; the next attempt is in `retry_in`. Values are on refresh
    /// only meanwhile.
    WebSocketLost {
        retry_in: Duration,
        reason: String,
    },
    Disconnected,
    Failed(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Out,
    In,
}

/// One OSC message, sent or received, for the traffic log.
#[derive(Debug, Clone, PartialEq)]
pub struct TrafficRecord {
    /// Monotonic.
    pub at: Instant,
    pub server_id: ServerId,
    pub direction: Direction,
    pub path: String,
    pub type_tags: String,
    pub args: Vec<OscType>,
}

/// Network to UI.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    ServerFound(DiscoveredServer),
    ServerChanged(DiscoveredServer),
    ServerStale(DiscoveredServer),
    /// `HOST_INFO` arrived. `destination` says where OSC goes, or why nothing can be sent.
    HostInfo {
        server_id: ServerId,
        info: HostInfo,
        destination: Result<OscDestination, SendBlocked>,
    },
    /// `HOST_INFO` failed: the server is browsable but not controllable.
    HostInfoUnavailable {
        server_id: ServerId,
        reason: String,
    },
    TreeLoaded {
        server_id: ServerId,
        raw: Arc<str>,
        report: ParseReport,
    },
    SubtreeLoaded {
        server_id: ServerId,
        path: String,
        raw: Arc<str>,
        report: ParseReport,
    },
    ValueChanged {
        server_id: ServerId,
        path: String,
        value: AttributeReply,
    },
    /// The server announced a new node; the parent's subtree is being reloaded.
    PathAdded {
        server_id: ServerId,
        path: String,
    },
    /// The server announced a removed node; the parent's subtree is being reloaded.
    PathRemoved {
        server_id: ServerId,
        path: String,
    },
    /// The server renamed a node; listens have moved, the parents are being reloaded.
    PathRenamed {
        server_id: ServerId,
        old: String,
        new: String,
    },
    TransportState {
        server_id: ServerId,
        state: TransportState,
    },
    TrafficLogged(TrafficRecord),
    Error {
        server_id: Option<ServerId>,
        error: UserFacingError,
    },
}

/// Called after every event, from a runtime thread.
pub type Notify = Arc<dyn Fn() + Send + Sync>;

struct Conn {
    client: OscQueryClient,
    destination: Option<Result<OscDestination, SendBlocked>>,
    generation: u64,
    /// Methods the UI wants values for. A live WebSocket session sends `LISTEN` for each.
    listens: HashSet<String>,
    /// Wakes the WebSocket task when `listens` changes. Dropped with the connection, which stops
    /// the task.
    ws_wake: Option<flume::Sender<()>>,
}

#[derive(Clone)]
struct Shared {
    conns: Arc<Mutex<HashMap<ServerId, Conn>>>,
    sender: Arc<tokio::sync::Mutex<OscUdpSender>>,
    events: flume::Sender<Event>,
    notify: Notify,
}

impl Shared {
    fn emit(&self, event: Event) {
        if self.events.send(event).is_ok() {
            (self.notify)();
        }
    }

    fn error(&self, server_id: ServerId, path: Option<String>, message: String) {
        self.emit(Event::Error {
            server_id: Some(server_id),
            error: UserFacingError { path, message },
        });
    }

    /// The client of a live connection, if `generation` is still current.
    fn client(&self, id: ServerId, generation: Option<u64>) -> Option<(OscQueryClient, u64)> {
        let conns = self.conns.lock().ok()?;
        let c = conns.get(&id)?;
        if generation.is_some_and(|g| g != c.generation) {
            return None;
        }
        Some((c.client.clone(), c.generation))
    }

    /// The wanted listens of a live connection, if `generation` is still current.
    fn listens(&self, id: ServerId, generation: u64) -> Option<HashSet<String>> {
        let conns = self.conns.lock().ok()?;
        let c = conns.get(&id).filter(|c| c.generation == generation)?;
        Some(c.listens.clone())
    }

    /// Changes the wanted listens of a connection and wakes its WebSocket task.
    fn edit_listens(&self, id: ServerId, edit: impl FnOnce(&mut HashSet<String>)) {
        if let Ok(mut conns) = self.conns.lock()
            && let Some(c) = conns.get_mut(&id)
        {
            edit(&mut c.listens);
            if let Some(w) = &c.ws_wake {
                let _ = w.send(());
            }
        }
    }
}

/// Starts the service on `runtime`. Returns the command sender. The service stops on
/// [`Command::Shutdown`] or when every sender is dropped.
pub fn spawn(
    runtime: &tokio::runtime::Handle,
    events: flume::Sender<Event>,
    notify: Notify,
) -> flume::Sender<Command> {
    let (tx, rx) = flume::unbounded();
    let shared = Shared {
        conns: Arc::new(Mutex::new(HashMap::new())),
        sender: Arc::new(tokio::sync::Mutex::new(OscUdpSender::new())),
        events,
        notify,
    };
    runtime.spawn(run(rx, shared));
    tx
}

async fn run(rx: flume::Receiver<Command>, shared: Shared) {
    let mut generation = 0u64;
    while let Ok(cmd) = rx.recv_async().await {
        match cmd {
            Command::Shutdown => break,
            Command::Connect {
                server_id,
                http_base,
            } => {
                generation += 1;
                let client = match OscQueryClient::new(http_base) {
                    Ok(c) => c,
                    Err(e) => {
                        shared.error(server_id, None, e.to_string());
                        shared.emit(Event::TransportState {
                            server_id,
                            state: TransportState::Failed(e.to_string()),
                        });
                        continue;
                    }
                };
                if let Ok(mut conns) = shared.conns.lock() {
                    conns.insert(
                        server_id,
                        Conn {
                            client,
                            destination: None,
                            generation,
                            listens: HashSet::new(),
                            ws_wake: None,
                        },
                    );
                }
                tokio::spawn(connect(shared.clone(), server_id, generation));
            }
            Command::Disconnect { server_id } => {
                let removed = shared
                    .conns
                    .lock()
                    .map(|mut c| c.remove(&server_id).is_some())
                    .unwrap_or(false);
                if removed {
                    shared.emit(Event::TransportState {
                        server_id,
                        state: TransportState::Disconnected,
                    });
                }
            }
            Command::RefreshTree {
                server_id,
                from_path,
            } => {
                tokio::spawn(refresh_tree(shared.clone(), server_id, from_path));
            }
            Command::RefreshValue { server_id, path } => {
                tokio::spawn(refresh_value(shared.clone(), server_id, path));
            }
            Command::SendOsc {
                server_id,
                path,
                signature,
                args,
                refresh_after,
            } => {
                tokio::spawn(send(
                    shared.clone(),
                    server_id,
                    path,
                    signature,
                    args,
                    refresh_after,
                ));
            }
            Command::Listen { server_id, path } => {
                shared.edit_listens(server_id, |l| {
                    l.insert(path);
                });
            }
            Command::Ignore { server_id, path } => {
                shared.edit_listens(server_id, |l| {
                    l.remove(&path);
                });
            }
        }
    }
}

async fn connect(shared: Shared, id: ServerId, generation: u64) {
    shared.emit(Event::TransportState {
        server_id: id,
        state: TransportState::Connecting,
    });
    let Some((client, _)) = shared.client(id, Some(generation)) else {
        return;
    };
    let mut ws_address = (client.http_host().to_owned(), client.http_port(), false);
    match client.fetch_host_info().await {
        Ok((info, _)) => {
            ws_address = info.websocket_address(client.http_host(), client.http_port());
            let destination = info.osc_destination(client.http_host(), client.http_port());
            if let Ok(mut conns) = shared.conns.lock() {
                match conns.get_mut(&id) {
                    Some(c) if c.generation == generation => {
                        c.destination = Some(destination.clone())
                    }
                    _ => return,
                }
            }
            shared.emit(Event::HostInfo {
                server_id: id,
                info,
                destination,
            });
        }
        Err(e) => shared.emit(Event::HostInfoUnavailable {
            server_id: id,
            reason: e.to_string(),
        }),
    }
    match client.fetch_tree("/").await {
        Ok(doc) => {
            if shared.client(id, Some(generation)).is_none() {
                return;
            }
            shared.emit(Event::TreeLoaded {
                server_id: id,
                raw: Arc::from(doc.raw),
                report: doc.report,
            });
            shared.emit(Event::TransportState {
                server_id: id,
                state: TransportState::HttpOnly,
            });
            let (host, port, advertised) = ws_address;
            websocket(shared, id, generation, ws::ws_url(&host, port), advertised).await;
        }
        Err(e) => {
            shared.error(id, Some("/".to_owned()), e.to_string());
            shared.emit(Event::TransportState {
                server_id: id,
                state: TransportState::Failed(e.to_string()),
            });
        }
    }
}

async fn refresh_tree(shared: Shared, id: ServerId, path: String) {
    let Some((client, generation)) = shared.client(id, None) else {
        return;
    };
    match client.fetch_tree(&path).await {
        Ok(doc) if shared.client(id, Some(generation)).is_some() => {
            shared.emit(Event::SubtreeLoaded {
                server_id: id,
                path,
                raw: Arc::from(doc.raw),
                report: doc.report,
            });
        }
        Ok(_) => {}
        Err(e) => shared.error(id, Some(path), e.to_string()),
    }
}

async fn refresh_value(shared: Shared, id: ServerId, path: String) {
    let Some((client, generation)) = shared.client(id, None) else {
        return;
    };
    match client.fetch_attribute(&path, "VALUE").await {
        Ok(value) if shared.client(id, Some(generation)).is_some() => {
            shared.emit(Event::ValueChanged {
                server_id: id,
                path,
                value,
            });
        }
        Ok(_) => {}
        Err(e) => shared.error(id, Some(path), e.to_string()),
    }
}

async fn send(
    shared: Shared,
    id: ServerId,
    path: OscPath,
    signature: TypeSignature,
    args: Vec<OscType>,
    refresh_after: bool,
) {
    let p = path.as_str().to_owned();
    let destination = shared
        .conns
        .lock()
        .ok()
        .and_then(|c| c.get(&id).map(|c| c.destination.clone()));
    let destination = match destination {
        None => return shared.error(id, Some(p), "not connected".to_owned()),
        Some(None) => {
            return shared.error(
                id,
                Some(p),
                "HOST_INFO unavailable: the server is not controllable".to_owned(),
            );
        }
        Some(Some(Err(blocked))) => return shared.error(id, Some(p), blocked.to_string()),
        Some(Some(Ok(d))) => d,
    };
    let args = match coerce_args(&signature, args) {
        Ok(a) => a,
        Err(e) => return shared.error(id, Some(p), e.to_string()),
    };
    let bytes = match encode_message(&path, args.clone()) {
        Ok(b) => b,
        Err(e) => return shared.error(id, Some(p), e.to_string()),
    };
    let sent = shared
        .sender
        .lock()
        .await
        .send_to(&destination, &bytes)
        .await;
    if let Err(e) = sent {
        return shared.error(id, Some(p), e.to_string());
    }
    shared.emit(Event::TrafficLogged(TrafficRecord {
        at: Instant::now(),
        server_id: id,
        direction: Direction::Out,
        path: p.clone(),
        type_tags: type_tags(&args),
        args,
    }));
    if refresh_after {
        refresh_value(shared, id, p).await;
    }
}

/// Why a WebSocket session ended.
enum SessionEnd {
    /// The connection was closed or replaced by the UI.
    Stopped,
    /// The socket failed or the server closed it.
    Lost(String),
}

/// The WebSocket of one connection, for as long as the connection lives. When the server did not
/// advertise `WS_IP` or `WS_PORT`, a single attempt is made on the HTTP address and its failure
/// is silent: values stay on refresh only. Once a session has been live, or when the address was
/// advertised, every loss is retried with backoff and every listen is sent again.
async fn websocket(shared: Shared, id: ServerId, generation: u64, url: String, advertised: bool) {
    let (wake_tx, wake) = flume::unbounded();
    match shared.conns.lock() {
        Ok(mut conns) => match conns.get_mut(&id) {
            Some(c) if c.generation == generation => c.ws_wake = Some(wake_tx),
            _ => return,
        },
        Err(_) => return,
    }
    let seed = std::collections::hash_map::RandomState::new();
    let mut attempt = 0u32;
    let mut live_before = false;
    loop {
        let reason = match open_websocket(&url).await {
            Ok(stream) => {
                if shared.client(id, Some(generation)).is_none() {
                    return;
                }
                attempt = 0;
                shared.emit(Event::TransportState {
                    server_id: id,
                    state: TransportState::WebSocketLive,
                });
                // Changes made while the socket was down were never pushed: reload once.
                if live_before {
                    tokio::spawn(refresh_tree(shared.clone(), id, "/".to_owned()));
                }
                live_before = true;
                match session(&shared, id, generation, stream, &wake).await {
                    SessionEnd::Stopped => return,
                    SessionEnd::Lost(r) => r,
                }
            }
            Err(e) if !advertised && !live_before => {
                tracing::debug!(%url, error = %e, "no WebSocket, values on refresh only");
                return;
            }
            Err(e) => e,
        };
        if shared.client(id, Some(generation)).is_none() {
            return;
        }
        let jitter = (seed.hash_one((id, attempt)) >> 11) as f64 / (1u64 << 53) as f64;
        let retry_in = ws::backoff(attempt, jitter);
        attempt = attempt.saturating_add(1);
        shared.emit(Event::TransportState {
            server_id: id,
            state: TransportState::WebSocketLost { retry_in, reason },
        });
        let deadline = tokio::time::Instant::now() + retry_in;
        loop {
            match tokio::time::timeout_at(deadline, wake.recv_async()).await {
                Err(_) => break,
                Ok(Err(_)) => return,
                Ok(Ok(())) => {}
            }
        }
    }
}

type WsStream =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn open_websocket(url: &str) -> Result<WsStream, String> {
    let config = WebSocketConfig::default()
        .max_message_size(Some(ws::MAX_MESSAGE))
        .max_frame_size(Some(ws::MAX_MESSAGE));
    let connect = tokio_tungstenite::connect_async_with_config(url, Some(config), true);
    match tokio::time::timeout(WS_CONNECT_TIMEOUT, connect).await {
        Ok(Ok((stream, _))) => Ok(stream),
        Ok(Err(e)) => Err(e.to_string()),
        Err(_) => Err(format!(
            "no answer within {} s",
            WS_CONNECT_TIMEOUT.as_secs()
        )),
    }
}

/// Same as the HTTP connect timeout.
const WS_CONNECT_TIMEOUT: Duration = Duration::from_secs(3);

/// One open WebSocket: keeps the server's listens equal to the wanted set, and turns what the
/// server sends into events.
async fn session(
    shared: &Shared,
    id: ServerId,
    generation: u64,
    stream: WsStream,
    wake: &flume::Receiver<()>,
) -> SessionEnd {
    let (mut sink, mut incoming) = stream.split();
    // What this server has been asked to send. Empty on a new socket: the server forgot.
    let mut sent: HashSet<String> = HashSet::new();
    let mut sync_needed = true;
    loop {
        if std::mem::take(&mut sync_needed) {
            let Some(wanted) = shared.listens(id, generation) else {
                let _ = sink.close().await;
                return SessionEnd::Stopped;
            };
            let mut frames: Vec<String> = wanted
                .difference(&sent)
                .map(|p| ws::listen_text(p))
                .collect();
            frames.extend(sent.difference(&wanted).map(|p| ws::ignore_text(p)));
            for f in frames {
                if let Err(e) = sink.feed(Message::text(f)).await {
                    return SessionEnd::Lost(e.to_string());
                }
            }
            if let Err(e) = sink.flush().await {
                return SessionEnd::Lost(e.to_string());
            }
            sent = wanted;
        }
        tokio::select! {
            w = wake.recv_async() => match w {
                Ok(()) => {
                    while wake.try_recv().is_ok() {}
                    sync_needed = true;
                }
                Err(_) => {
                    let _ = sink.close().await;
                    return SessionEnd::Stopped;
                }
            },
            m = incoming.next() => match m {
                None | Some(Ok(Message::Close(_))) => {
                    return SessionEnd::Lost("closed by the server".to_owned());
                }
                Some(Err(e)) => return SessionEnd::Lost(e.to_string()),
                Some(Ok(Message::Text(t))) => on_text(shared, id, t.as_str(), &mut sent),
                Some(Ok(Message::Binary(b))) => on_binary(shared, id, &b),
                Some(Ok(_)) => {}
            },
        }
    }
}

/// A structure command: reload what it names, and keep listens where the server keeps them.
fn on_text(shared: &Shared, id: ServerId, text: &str, sent: &mut HashSet<String>) {
    let command = match ws::parse_server_text(text) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(error = %e, "WebSocket text frame ignored");
            return;
        }
    };
    let reload = |path: String| {
        tokio::spawn(refresh_tree(shared.clone(), id, path));
    };
    match command {
        ServerCommand::PathChanged(path) => reload(path),
        ServerCommand::PathAdded(path) => {
            reload(ws::parent_path(&path));
            shared.emit(Event::PathAdded {
                server_id: id,
                path,
            });
        }
        ServerCommand::PathRemoved(path) => {
            // The server forgets listens on removed methods.
            sent.retain(|p| !ws::is_within(p, &path));
            shared.edit_listens(id, |l| l.retain(|p| !ws::is_within(p, &path)));
            reload(ws::parent_path(&path));
            shared.emit(Event::PathRemoved {
                server_id: id,
                path,
            });
        }
        ServerCommand::PathRenamed { old, new } => {
            // Listens follow the rename on both sides (proposal, PATH_RENAMED).
            let rename = |set: &mut HashSet<String>| {
                *set = set
                    .drain()
                    .map(|p| ws::renamed(&p, &old, &new).unwrap_or(p))
                    .collect();
            };
            rename(sent);
            shared.edit_listens(id, rename);
            let (from, to) = (ws::parent_path(&old), ws::parent_path(&new));
            if from != to {
                reload(from);
            }
            reload(to);
            shared.emit(Event::PathRenamed {
                server_id: id,
                old,
                new,
            });
        }
    }
}

/// An OSC packet: each message is a value update for its path, and is logged.
fn on_binary(shared: &Shared, id: ServerId, bytes: &[u8]) {
    let messages = match ws::decode_osc(bytes) {
        Ok(m) => m,
        Err(e) => {
            tracing::warn!(error = %e, "WebSocket binary frame ignored");
            return;
        }
    };
    for m in messages {
        shared.emit(Event::ValueChanged {
            server_id: id,
            path: m.addr.clone(),
            value: AttributeReply::Value(ws::args_to_json(&m.args)),
        });
        shared.emit(Event::TrafficLogged(TrafficRecord {
            at: Instant::now(),
            server_id: id,
            direction: Direction::In,
            path: m.addr,
            type_tags: type_tags(&m.args),
            args: m.args,
        }));
    }
}

/// The type tag string of encoded arguments, `[` and `]` around arrays.
pub fn type_tags(args: &[OscType]) -> String {
    let mut out = String::new();
    for a in args {
        match a {
            OscType::Int(_) => out.push('i'),
            OscType::Float(_) => out.push('f'),
            OscType::String(_) => out.push('s'),
            OscType::Blob(_) => out.push('b'),
            OscType::Time(_) => out.push('t'),
            OscType::Long(_) => out.push('h'),
            OscType::Double(_) => out.push('d'),
            OscType::Char(_) => out.push('c'),
            OscType::Color(_) => out.push('r'),
            OscType::Midi(_) => out.push('m'),
            OscType::Bool(true) => out.push('T'),
            OscType::Bool(false) => out.push('F'),
            OscType::Array(a) => {
                out.push('[');
                out.push_str(&type_tags(&a.content));
                out.push(']');
            }
            OscType::Nil => out.push('N'),
            OscType::Inf => out.push('I'),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use rosc::OscArray;

    #[test]
    fn tags_of_encoded_arguments() {
        let args = vec![
            OscType::Int(1),
            OscType::Array(OscArray {
                content: vec![OscType::Float(1.0), OscType::String("x".into())],
            }),
            OscType::Bool(false),
            OscType::Inf,
            OscType::Nil,
        ];
        assert_eq!(type_tags(&args), "i[fs]FIN");
    }
}

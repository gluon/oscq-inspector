//! The network half of the application: commands in, events out, all I/O on a `tokio` runtime.
//!
//! The UI sends [`Command`]s on a `flume` channel and drains [`Event`]s from another. After each
//! event the service calls a notify callback, which the UI uses to request a repaint. Nothing here
//! knows about the UI toolkit.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use rosc::OscType;
use url::Url;

use crate::client::{AttributeReply, OscQueryClient};
use crate::document::ParseReport;
use crate::host_info::{HostInfo, OscDestination, SendBlocked};
use crate::osc::{OscUdpSender, coerce_args, encode_message};
use crate::path::OscPath;
use crate::server::{DiscoveredServer, ServerId};
use crate::types::TypeSignature;

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
    /// Tree loaded over HTTP. No WebSocket in this version: values on refresh only.
    HttpOnly,
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
    match client.fetch_host_info().await {
        Ok((info, _)) => {
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

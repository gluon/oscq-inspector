//! The WebSocket part of OSCQuery, without I/O: text commands both ways, binary OSC frames, and
//! the reconnection delay. The connection itself is driven by [`crate::service`].

use std::time::Duration;

use rosc::{OscMessage, OscPacket, OscType};
use serde_json::{Value, json};

use crate::document::{DocumentError, parse_json};
use crate::types::MAX_ARRAY_DEPTH;

/// Largest WebSocket message accepted from a server. OSCQuery text commands carry one path, and
/// a binary frame one OSC packet.
pub const MAX_MESSAGE: usize = 4 << 20;

/// Bundles nested in bundles, and `[` groups in a type tag string, deeper than this are refused
/// before decoding: `rosc` decodes bundles by recursion.
pub const MAX_OSC_NESTING: usize = MAX_ARRAY_DEPTH;

/// First reconnection delay, doubled on each failure.
pub const BACKOFF_START: Duration = Duration::from_millis(500);
/// Longest reconnection delay.
pub const BACKOFF_CAP: Duration = Duration::from_secs(15);

/// `LISTEN` for one path, as a text frame.
pub fn listen_text(path: &str) -> String {
    json!({"COMMAND": "LISTEN", "DATA": path}).to_string()
}

/// `IGNORE` for one path, as a text frame.
pub fn ignore_text(path: &str) -> String {
    json!({"COMMAND": "IGNORE", "DATA": path}).to_string()
}

/// A text command from the server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServerCommand {
    /// Structure changed at or below the path: reload its subtree.
    PathChanged(String),
    PathAdded(String),
    PathRemoved(String),
    PathRenamed {
        old: String,
        new: String,
    },
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum WsError {
    #[error("text frame is not a JSON object: {0}")]
    NotJson(String),
    #[error("text frame has no string COMMAND")]
    NoCommand,
    #[error("unknown COMMAND {0:?}")]
    UnknownCommand(String),
    #[error("{0}: DATA is not what the proposal defines")]
    BadData(String),
    #[error("OSC packet nested deeper than {MAX_OSC_NESTING} levels")]
    TooDeep,
    #[error("binary frame is not an OSC packet: {0}")]
    Osc(String),
}

impl From<DocumentError> for WsError {
    fn from(e: DocumentError) -> Self {
        WsError::NotJson(e.to_string())
    }
}

/// Parses a server text frame: `{"COMMAND": ..., "DATA": ...}`. `DATA` is a path for every
/// command but `PATH_RENAMED`, whose `DATA` is `{"OLD": path, "NEW": path}`.
pub fn parse_server_text(text: &str) -> Result<ServerCommand, WsError> {
    let Value::Object(mut obj) = parse_json(text.as_bytes())? else {
        return Err(WsError::NotJson("not an object".to_owned()));
    };
    let Some(Value::String(command)) = obj.remove("COMMAND") else {
        return Err(WsError::NoCommand);
    };
    let data = obj.remove("DATA").unwrap_or(Value::Null);
    let path = |v: &Value| match v {
        Value::String(p) if p.starts_with('/') => Some(p.clone()),
        _ => None,
    };
    let bad = || WsError::BadData(command.clone());
    match command.as_str() {
        "PATH_CHANGED" => path(&data).map(ServerCommand::PathChanged).ok_or_else(bad),
        "PATH_ADDED" => path(&data).map(ServerCommand::PathAdded).ok_or_else(bad),
        "PATH_REMOVED" => path(&data).map(ServerCommand::PathRemoved).ok_or_else(bad),
        "PATH_RENAMED" => match (
            data.get("OLD").and_then(path),
            data.get("NEW").and_then(path),
        ) {
            (Some(old), Some(new)) => Ok(ServerCommand::PathRenamed { old, new }),
            _ => Err(bad()),
        },
        _ => Err(WsError::UnknownCommand(command)),
    }
}

/// The OSC messages of one binary frame, bundles flattened in order. Nesting is checked without
/// recursion first, so a hostile frame cannot exhaust the stack.
pub fn decode_osc(bytes: &[u8]) -> Result<Vec<OscMessage>, WsError> {
    check_nesting(bytes, MAX_OSC_NESTING)?;
    let (_, packet) = rosc::decoder::decode_udp(bytes).map_err(|e| WsError::Osc(e.to_string()))?;
    let mut out = Vec::new();
    let mut stack = vec![packet];
    while let Some(p) = stack.pop() {
        match p {
            OscPacket::Message(m) => out.push(m),
            OscPacket::Bundle(b) => stack.extend(b.content.into_iter().rev()),
        }
    }
    Ok(out)
}

/// Walks an OSC packet without recursion: bundle elements, then the `[` depth of each message's
/// type tags. Anything malformed is left to the decoder, which rejects it.
fn check_nesting(bytes: &[u8], max: usize) -> Result<(), WsError> {
    const BUNDLE: &[u8] = b"#bundle\0";
    // Remaining elements of each open bundle; its length is the current bundle depth.
    let mut open: Vec<&[u8]> = Vec::new();
    let mut next = Some(bytes);
    loop {
        if let Some(packet) = next.take() {
            if packet.starts_with(BUNDLE) {
                if open.len() >= max {
                    return Err(WsError::TooDeep);
                }
                // The tag, then an 8 byte time tag, then the elements.
                open.push(packet.get(16..).unwrap_or(&[]));
            } else if tag_depth(packet) > max {
                return Err(WsError::TooDeep);
            }
        }
        let Some(rest) = open.last_mut() else {
            return Ok(());
        };
        let size = rest
            .get(..4)
            .map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]) as usize);
        match size {
            Some(n) if rest.len() - 4 >= n => {
                next = Some(&rest[4..4 + n]);
                *rest = &rest[4 + n..];
            }
            _ => {
                open.pop();
            }
        }
    }
}

/// Deepest `[` nesting in a message's type tag string, 0 when it cannot be found.
fn tag_depth(message: &[u8]) -> usize {
    let Some(end) = message.iter().position(|b| *b == 0) else {
        return 0;
    };
    // The address is padded with zeros to a multiple of 4 bytes.
    let tags_at = (end + 4) & !3;
    let tags = message.get(tags_at..).unwrap_or(&[]);
    let tags = &tags[..tags.iter().position(|b| *b == 0).unwrap_or(tags.len())];
    let (mut depth, mut deepest) = (0usize, 0usize);
    for t in tags {
        match t {
            b'[' => {
                depth += 1;
                deepest = deepest.max(depth);
            }
            b']' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    deepest
}

/// OSC arguments as a `VALUE` array, with the JSON encoding the proposal gives each type: numbers
/// for `i f h d t`, strings for `s c`, `#RRGGBBAA` for `r`, booleans for `T F`, nested arrays for
/// `[ ]`, and null for `b m N I`.
pub fn args_to_json(args: &[OscType]) -> Value {
    Value::Array(args.iter().map(arg_to_json).collect())
}

fn arg_to_json(a: &OscType) -> Value {
    match a {
        OscType::Int(v) => json!(v),
        OscType::Long(v) => json!(v),
        OscType::Float(v) => json!(v),
        OscType::Double(v) => json!(v),
        OscType::Time(t) => json!(f64::from(t.seconds) + f64::from(t.fractional) / 4_294_967_296.0),
        OscType::String(s) => json!(s),
        OscType::Char(c) => json!(c.to_string()),
        OscType::Color(c) => json!(format!(
            "#{:02X}{:02X}{:02X}{:02X}",
            c.red, c.green, c.blue, c.alpha
        )),
        OscType::Bool(b) => json!(b),
        OscType::Array(a) => args_to_json(&a.content),
        OscType::Blob(_) | OscType::Midi(_) | OscType::Nil | OscType::Inf => Value::Null,
    }
}

/// Delay before reconnection attempt `attempt`, counted from 0: 500 ms doubled each time, capped
/// at 15 s, minus up to a fifth at random. `jitter` is uniform in `[0, 1)`.
pub fn backoff(attempt: u32, jitter: f64) -> Duration {
    let base = BACKOFF_START
        .saturating_mul(1u32.checked_shl(attempt.min(31)).unwrap_or(u32::MAX))
        .min(BACKOFF_CAP);
    base.mul_f64(1.0 - 0.2 * jitter.clamp(0.0, 1.0))
}

/// `ws://host:port/`, with brackets around an IPv6 host.
pub fn ws_url(host: &str, port: u16) -> String {
    if host.contains(':') {
        format!("ws://[{host}]:{port}/")
    } else {
        format!("ws://{host}:{port}/")
    }
}

/// The container holding `path`: `/a/b` gives `/a`, `/a` gives `/`.
pub fn parent_path(path: &str) -> String {
    match path.trim_end_matches('/').rsplit_once('/') {
        Some(("", _)) | None => "/".to_owned(),
        Some((parent, _)) => parent.to_owned(),
    }
}

/// `path` moved from under `old` to under `new`, or `None` when it is not `old` or below it.
pub fn renamed(path: &str, old: &str, new: &str) -> Option<String> {
    if path == old {
        return Some(new.to_owned());
    }
    let rest = path.strip_prefix(old)?.strip_prefix('/')?;
    Some(format!("{}/{rest}", new.trim_end_matches('/')))
}

/// Whether `path` is `root` or below it.
pub fn is_within(path: &str, root: &str) -> bool {
    root == "/" || path == root || path.strip_prefix(root).is_some_and(|r| r.starts_with('/'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rosc::{OscArray, OscBundle, OscColor, OscTime};

    #[test]
    fn commands_both_ways() {
        let l: Value = serde_json::from_str(&listen_text("/a b")).unwrap();
        assert_eq!(l, json!({"COMMAND":"LISTEN","DATA":"/a b"}));
        let i: Value = serde_json::from_str(&ignore_text("/x")).unwrap();
        assert_eq!(i, json!({"COMMAND":"IGNORE","DATA":"/x"}));

        let p = |s: &str| parse_server_text(s);
        assert_eq!(
            p(r#"{"COMMAND":"PATH_CHANGED","DATA":"/foo"}"#),
            Ok(ServerCommand::PathChanged("/foo".into()))
        );
        assert_eq!(
            p(r#"{"COMMAND":"PATH_ADDED","DATA":"/foo/new"}"#),
            Ok(ServerCommand::PathAdded("/foo/new".into()))
        );
        assert_eq!(
            p(r#"{"COMMAND":"PATH_REMOVED","DATA":"/foo"}"#),
            Ok(ServerCommand::PathRemoved("/foo".into()))
        );
        assert_eq!(
            p(r#"{"DATA":{"OLD":"/a","NEW":"/b"},"COMMAND":"PATH_RENAMED"}"#),
            Ok(ServerCommand::PathRenamed {
                old: "/a".into(),
                new: "/b".into()
            })
        );
        assert_eq!(
            p(r#"{"COMMAND":"PATH_RENAMED","DATA":"/a"}"#),
            Err(WsError::BadData("PATH_RENAMED".into()))
        );
        assert_eq!(
            p(r#"{"COMMAND":"PATH_ADDED","DATA":3}"#),
            Err(WsError::BadData("PATH_ADDED".into()))
        );
        assert_eq!(
            p(r#"{"COMMAND":"HELLO"}"#),
            Err(WsError::UnknownCommand("HELLO".into()))
        );
        assert_eq!(p(r#"{"DATA":"/a"}"#), Err(WsError::NoCommand));
        assert!(matches!(p("not json"), Err(WsError::NotJson(_))));
        assert!(matches!(p(&"[".repeat(100_000)), Err(WsError::NotJson(_))));
    }

    fn encode(p: &OscPacket) -> Vec<u8> {
        rosc::encoder::encode(p).unwrap()
    }

    fn msg(addr: &str, args: Vec<OscType>) -> OscPacket {
        OscPacket::Message(OscMessage {
            addr: addr.into(),
            args,
        })
    }

    fn bundle(content: Vec<OscPacket>) -> OscPacket {
        OscPacket::Bundle(OscBundle {
            timetag: OscTime {
                seconds: 0,
                fractional: 1,
            },
            content,
        })
    }

    #[test]
    fn frames_decode_with_bundles_flattened_in_order() {
        let one = encode(&msg("/a", vec![OscType::Float(0.5)]));
        assert_eq!(
            decode_osc(&one).unwrap(),
            vec![OscMessage {
                addr: "/a".into(),
                args: vec![OscType::Float(0.5)]
            }]
        );
        let nested = encode(&bundle(vec![
            msg("/a", vec![OscType::Int(1)]),
            bundle(vec![msg("/b", vec![]), msg("/c", vec![])]),
            msg("/d", vec![]),
        ]));
        let addrs: Vec<String> = decode_osc(&nested)
            .unwrap()
            .into_iter()
            .map(|m| m.addr)
            .collect();
        assert_eq!(addrs, ["/a", "/b", "/c", "/d"]);
        assert!(matches!(decode_osc(b"garbage"), Err(WsError::Osc(_))));
        assert!(matches!(decode_osc(&[]), Err(WsError::Osc(_))));
    }

    /// Built by hand: the encoder would recurse as deep as the decoder.
    fn deep_bundle(depth: usize) -> Vec<u8> {
        const HEADER: &[u8] = b"#bundle\0\0\0\0\0\0\0\0\x01";
        let inner = encode(&msg("/x", vec![]));
        let mut p = Vec::with_capacity(depth * (HEADER.len() + 4) + inner.len());
        for level in (0..depth).rev() {
            let element = inner.len() + level * (HEADER.len() + 4);
            p.extend_from_slice(HEADER);
            p.extend_from_slice(&(element as u32).to_be_bytes());
        }
        p.extend_from_slice(&inner);
        p
    }

    #[test]
    fn hostile_nesting_is_refused_without_recursion() {
        assert_eq!(decode_osc(&deep_bundle(MAX_OSC_NESTING)).unwrap().len(), 1);
        assert_eq!(
            decode_osc(&deep_bundle(MAX_OSC_NESTING + 1)),
            Err(WsError::TooDeep)
        );
        // Deep enough to overflow a recursive decoder; checked on a small stack.
        let deep = deep_bundle(200_000);
        let r = std::thread::Builder::new()
            .stack_size(256 * 1024)
            .spawn(move || decode_osc(&deep))
            .unwrap()
            .join()
            .unwrap();
        assert_eq!(r, Err(WsError::TooDeep));

        let mut tags = String::from(",");
        tags.push_str(&"[".repeat(100_000));
        tags.push_str(&"]".repeat(100_000));
        let mut m = b"/x\0\0".to_vec();
        m.extend_from_slice(tags.as_bytes());
        m.extend(std::iter::repeat_n(0u8, 4 - tags.len() % 4));
        let r = std::thread::Builder::new()
            .stack_size(256 * 1024)
            .spawn(move || decode_osc(&m))
            .unwrap()
            .join()
            .unwrap();
        assert_eq!(r, Err(WsError::TooDeep));

        let ok = encode(&msg(
            "/x",
            vec![OscType::Array(OscArray {
                content: vec![OscType::Int(1)],
            })],
        ));
        assert_eq!(decode_osc(&ok).unwrap().len(), 1);
    }

    #[test]
    fn values_as_json() {
        let v = args_to_json(&[
            OscType::Int(1),
            OscType::Double(0.5),
            OscType::String("x".into()),
            OscType::Bool(false),
            OscType::Color(OscColor {
                red: 255,
                green: 0,
                blue: 16,
                alpha: 128,
            }),
            OscType::Array(OscArray {
                content: vec![OscType::Float(1.0), OscType::Nil],
            }),
            OscType::Inf,
        ]);
        assert_eq!(
            v,
            json!([1, 0.5, "x", false, "#FF001080", [1.0, null], null])
        );
    }

    #[test]
    fn backoff_starts_at_500_ms_doubles_and_caps_at_15_s() {
        let ms = |a, j| backoff(a, j).as_millis();
        assert_eq!(ms(0, 0.0), 500);
        assert_eq!(ms(1, 0.0), 1000);
        assert_eq!(ms(4, 0.0), 8000);
        assert_eq!(ms(5, 0.0), 15000);
        assert_eq!(ms(200, 0.0), 15000);
        assert_eq!(ms(0, 0.5), 450);
        assert_eq!(ms(200, 0.999_999), 12000);
    }

    #[test]
    fn paths() {
        assert_eq!(ws_url("10.0.0.2", 80), "ws://10.0.0.2:80/");
        assert_eq!(ws_url("::1", 80), "ws://[::1]:80/");
        assert_eq!(parent_path("/a/b"), "/a");
        assert_eq!(parent_path("/a"), "/");
        assert_eq!(parent_path("/"), "/");
        assert_eq!(renamed("/a", "/a", "/z").as_deref(), Some("/z"));
        assert_eq!(renamed("/a/b/c", "/a", "/z").as_deref(), Some("/z/b/c"));
        assert_eq!(renamed("/ab", "/a", "/z"), None);
        assert!(is_within("/a/b", "/a") && is_within("/a", "/a") && is_within("/a", "/"));
        assert!(!is_within("/ab", "/a"));
    }
}

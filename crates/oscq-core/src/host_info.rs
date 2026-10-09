//! `HOST_INFO` and the OSC destination derived from it.

use std::collections::BTreeMap;

use serde_json::{Map, Value};

use crate::document::{Diagnostic, DiagnosticKind, DocumentError, parse_json};

/// `OSC_TRANSPORT`. Absent means UDP.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OscTransport {
    Udp,
    Tcp,
}

/// The reply to a `HOST_INFO` query. Every key is optional.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct HostInfo {
    pub name: Option<String>,
    /// `EXTENSIONS`, attribute or command name to support flag. Absent means unsupported.
    pub extensions: BTreeMap<String, bool>,
    pub osc_ip: Option<String>,
    /// `OSC_PORT` when present and valid.
    pub osc_port: Option<u16>,
    /// `OSC_PORT` was present but not a valid port. The server is then not controllable.
    pub osc_port_invalid: bool,
    pub osc_transport: Option<OscTransport>,
    pub ws_ip: Option<String>,
    pub ws_port: Option<u16>,
    /// Unknown keys, and known keys whose value had the wrong JSON type, kept verbatim.
    pub extra: Map<String, Value>,
}

/// Where the OSC port of an [`OscDestination`] came from. Shown in the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortSource {
    /// `OSC_PORT` in `HOST_INFO`.
    Advertised,
    /// `OSC_PORT` absent, the HTTP port is used, as the proposal specifies.
    InferredFromHttp,
}

/// Why nothing can be sent to a server.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SendBlocked {
    #[error("the server declares OSC_TRANSPORT TCP, which this version does not send to")]
    TcpTransport,
    #[error("the server's OSC_PORT is not a valid port")]
    InvalidOscPort,
}

/// The OSC receiver of a server, resolved from `HOST_INFO` and the HTTP address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OscDestination {
    pub host: String,
    pub port: u16,
    pub port_source: PortSource,
    pub transport: OscTransport,
}

impl OscDestination {
    /// `Ok` when this client can send to the destination.
    pub fn check_sendable(&self) -> Result<(), SendBlocked> {
        match self.transport {
            OscTransport::Udp => Ok(()),
            OscTransport::Tcp => Err(SendBlocked::TcpTransport),
        }
    }
}

impl HostInfo {
    /// Parses a `HOST_INFO` reply. Problems that do not prevent parsing are returned as
    /// diagnostics against the path `/`.
    pub fn parse(bytes: &[u8]) -> Result<(HostInfo, Vec<Diagnostic>), DocumentError> {
        let Value::Object(mut obj) = parse_json(bytes)? else {
            return Err(DocumentError::NotAnObject);
        };
        let mut info = HostInfo::default();
        let mut diags = Vec::new();

        if let Some(v) = obj.remove("NAME") {
            match v {
                Value::String(s) => info.name = Some(s),
                other => bad(&mut diags, &mut info, "NAME", other),
            }
        }
        if let Some(v) = obj.remove("EXTENSIONS") {
            match v {
                Value::Object(map) => {
                    for (k, flag) in map {
                        match flag {
                            Value::Bool(b) => {
                                info.extensions.insert(k, b);
                            }
                            other => bad(&mut diags, &mut info, &format!("EXTENSIONS.{k}"), other),
                        }
                    }
                }
                other => bad(&mut diags, &mut info, "EXTENSIONS", other),
            }
        }
        if let Some(v) = obj.remove("OSC_IP") {
            match v {
                Value::String(s) => info.osc_ip = Some(s),
                other => bad(&mut diags, &mut info, "OSC_IP", other),
            }
        }
        if let Some(v) = obj.remove("WS_IP") {
            match v {
                Value::String(s) => info.ws_ip = Some(s),
                other => bad(&mut diags, &mut info, "WS_IP", other),
            }
        }
        if let Some(v) = obj.remove("OSC_PORT") {
            match port(&v) {
                Some(p) => info.osc_port = Some(p),
                None => {
                    info.osc_port_invalid = true;
                    bad(&mut diags, &mut info, "OSC_PORT", v);
                }
            }
        }
        if let Some(v) = obj.remove("WS_PORT") {
            match port(&v) {
                Some(p) => info.ws_port = Some(p),
                None => bad(&mut diags, &mut info, "WS_PORT", v),
            }
        }
        if let Some(v) = obj.remove("OSC_TRANSPORT") {
            match &v {
                Value::String(s) if s.eq_ignore_ascii_case("TCP") => {
                    info.osc_transport = Some(OscTransport::Tcp)
                }
                Value::String(s) if s.eq_ignore_ascii_case("UDP") => {
                    info.osc_transport = Some(OscTransport::Udp)
                }
                _ => bad(&mut diags, &mut info, "OSC_TRANSPORT", v),
            }
        }
        info.extra.extend(obj);
        Ok((info, diags))
    }

    /// True only when `EXTENSIONS` lists `key` as `true`.
    pub fn supports(&self, key: &str) -> bool {
        self.extensions.get(key).copied().unwrap_or(false)
    }

    /// The OSC receiver, applying the proposal's defaults: `OSC_IP` absent means the HTTP host,
    /// `OSC_PORT` absent means the HTTP port, `OSC_TRANSPORT` absent means UDP.
    pub fn osc_destination(
        &self,
        http_host: &str,
        http_port: u16,
    ) -> Result<OscDestination, SendBlocked> {
        if self.osc_port_invalid {
            return Err(SendBlocked::InvalidOscPort);
        }
        let (port, port_source) = match self.osc_port {
            Some(p) => (p, PortSource::Advertised),
            None => (http_port, PortSource::InferredFromHttp),
        };
        Ok(OscDestination {
            host: self.osc_ip.clone().unwrap_or_else(|| http_host.to_owned()),
            port,
            port_source,
            transport: self.osc_transport.clone().unwrap_or(OscTransport::Udp),
        })
    }

    /// The WebSocket address: `WS_IP` and `WS_PORT`, each defaulting to the HTTP host and port.
    /// The flag tells whether either was given by the server.
    pub fn websocket_address(&self, http_host: &str, http_port: u16) -> (String, u16, bool) {
        let advertised = self.ws_ip.is_some() || self.ws_port.is_some();
        (
            self.ws_ip.clone().unwrap_or_else(|| http_host.to_owned()),
            self.ws_port.unwrap_or(http_port),
            advertised,
        )
    }
}

fn bad(diags: &mut Vec<Diagnostic>, info: &mut HostInfo, key: &str, v: Value) {
    diags.push(Diagnostic {
        path: "/".to_owned(),
        kind: DiagnosticKind::InvalidAttribute(key.to_owned()),
    });
    info.extra.insert(key.to_owned(), v);
}

/// A port given as a JSON number or as a numeric string, as VVOSCQueryProtocol accepts.
fn port(v: &Value) -> Option<u16> {
    match v {
        Value::Number(n) => n.as_u64().and_then(|p| u16::try_from(p).ok()),
        Value::String(s) => s.trim().parse::<u16>().ok(),
        _ => None,
    }
    .filter(|p| *p != 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn port_forms() {
        assert_eq!(port(&Value::from(9000)), Some(9000));
        assert_eq!(port(&Value::from("9000")), Some(9000));
        assert_eq!(port(&Value::from(70000)), None);
        assert_eq!(port(&Value::from(0)), None);
        assert_eq!(port(&Value::from(-1)), None);
        assert_eq!(port(&Value::from(9000.5)), None);
        assert_eq!(port(&Value::Bool(true)), None);
    }
}

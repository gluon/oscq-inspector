//! OSCQuery protocol client: address space types, HTTP client, WebSocket client and OSC sending.
//!
//! Must never depend on `egui` or `eframe`.

pub mod client;
pub mod document;
pub mod host_info;
pub mod model;
pub mod osc;
pub mod path;
pub mod server;
pub mod types;

pub use client::{AttributeReply, ClientConfig, ClientError, Document, OscQueryClient};
pub use document::{Diagnostic, DiagnosticKind, DocumentError, ParseReport, parse_document};
pub use host_info::{HostInfo, OscDestination, OscTransport, PortSource, SendBlocked};
pub use model::{Access, ArgEntry, ClipMode, Node, PerArg, Range};
pub use osc::{CoerceError, OscError, OscUdpSender};
pub use path::{OscPath, PathError};
pub use server::{
    AddressClass, DiscoveredServer, InterfaceRef, Origin, ServerAddress, ServerId, ServiceKind,
};
pub use types::{ArgType, TypeError, TypeSignature, TypeTag};

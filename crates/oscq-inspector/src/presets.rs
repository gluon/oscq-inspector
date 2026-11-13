//! Presets (PRD 9.9): sixteen slots, each the values of a server's writable methods, captured in
//! one click and recalled in one click, against the same server or another.

use oscq_core::model::{ArgEntry, Node};
use oscq_core::osc::arg_from_text;
use oscq_core::{ArgType, OscPath, TypeSignature, TypeTag};
use rosc::{OscColor, OscType};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::state::TreeState;

pub const SLOTS: usize = 16;

/// One slot: where it was captured, and each method's value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Preset {
    /// `host:port` of the server it was captured from.
    pub server: String,
    pub server_name: String,
    pub values: Vec<PresetValue>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PresetValue {
    pub path: String,
    /// The `TYPE` of the method when captured; a recall matches on it.
    pub type_tags: String,
    pub args: Vec<PresetArg>,
}

/// An argument as kept in the config, before it is coerced to the method's type on sending.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PresetArg {
    Int(i64),
    Float(f64),
    Bool(bool),
    Text(String),
    Color([u8; 4]),
}

impl PresetArg {
    fn from_osc(a: &OscType) -> Option<Self> {
        Some(match a {
            OscType::Int(v) => Self::Int(i64::from(*v)),
            OscType::Long(v) => Self::Int(*v),
            OscType::Float(v) => Self::Float(f64::from(*v)),
            OscType::Double(v) => Self::Float(*v),
            OscType::Bool(v) => Self::Bool(*v),
            OscType::String(s) => Self::Text(s.clone()),
            OscType::Char(c) => Self::Text(c.to_string()),
            OscType::Color(c) => Self::Color([c.red, c.green, c.blue, c.alpha]),
            _ => return None,
        })
    }

    fn to_osc(&self) -> OscType {
        match self {
            Self::Int(v) => OscType::Long(*v),
            Self::Float(v) => OscType::Double(*v),
            Self::Bool(v) => OscType::Bool(*v),
            Self::Text(s) => OscType::String(s.clone()),
            Self::Color([red, green, blue, alpha]) => OscType::Color(OscColor {
                red: *red,
                green: *green,
                blue: *blue,
                alpha: *alpha,
            }),
        }
    }
}

/// What a capture took, and how many methods in its scope had no value to keep.
#[derive(Debug, Clone, PartialEq)]
pub struct Capture {
    pub preset: Preset,
    pub skipped: usize,
}

/// The methods a capture covers: the pinned ones when there are any, else every method of the
/// loaded tree, in path order.
fn scope<'a>(tree: &'a TreeState, pinned: &[String]) -> Vec<&'a Node> {
    let mut paths: Vec<&String> = if pinned.is_empty() {
        tree.paths.keys().collect()
    } else {
        pinned.iter().collect()
    };
    paths.sort();
    paths.dedup();
    paths
        .into_iter()
        .filter_map(|p| tree.node(p))
        .filter(|n| n.is_method())
        .collect()
}

/// Captures the writable methods of `tree` in scope: each one's value sent and not yet
/// confirmed if any, else its value read. A method with neither, or whose type cannot be kept
/// (impulse, nil, array, blob and the like), is skipped and counted.
pub fn capture(
    server: &str,
    server_name: &str,
    tree: &TreeState,
    value_supported: bool,
    pinned: &[String],
) -> Capture {
    let mut values = Vec::new();
    let mut skipped = 0;
    for node in scope(tree, pinned) {
        if !node.effective_access(value_supported).writable() {
            continue;
        }
        match value_of(tree, node) {
            Some(v) => values.push(v),
            None => skipped += 1,
        }
    }
    Capture {
        preset: Preset {
            server: server.to_owned(),
            server_name: server_name.to_owned(),
            values,
        },
        skipped,
    }
}

fn value_of(tree: &TreeState, node: &Node) -> Option<PresetValue> {
    let sig = node.signature.as_ref()?;
    let tags = node.type_raw.clone()?;
    if sig.args().is_empty() || !sig.args().iter().all(keepable) {
        return None;
    }
    let args: Vec<OscType> = match tree.pending.get(&node.full_path) {
        Some(sent) => sent.clone(),
        None => read_args(node, sig)?,
    };
    if args.len() != sig.args().len() {
        return None;
    }
    Some(PresetValue {
        path: node.full_path.clone(),
        type_tags: tags,
        args: args
            .iter()
            .map(PresetArg::from_osc)
            .collect::<Option<_>>()?,
    })
}

/// A type a preset can hold and send back: a single value, not an action or a blob.
fn keepable(ty: &ArgType) -> bool {
    matches!(
        ty,
        ArgType::Tag(
            TypeTag::Int32
                | TypeTag::Int64
                | TypeTag::Float32
                | TypeTag::Double
                | TypeTag::String
                | TypeTag::Char
                | TypeTag::Color
                | TypeTag::True
                | TypeTag::False
        )
    )
}

/// The node's `VALUE` as arguments of its type, each entry parsed as the raw entry field would.
fn read_args(node: &Node, sig: &TypeSignature) -> Option<Vec<OscType>> {
    let value = node.value.as_ref()?;
    sig.args()
        .iter()
        .enumerate()
        .map(|(i, ty)| {
            let text = match value.entry(i)? {
                ArgEntry::Value(Value::Number(n)) => n.to_string(),
                ArgEntry::Value(Value::String(s)) => s.clone(),
                ArgEntry::Value(Value::Bool(b)) => b.to_string(),
                _ => return None,
            };
            arg_from_text(ty, &text).ok()
        })
        .collect()
}

/// One message of a recall, for the method at `path`.
#[derive(Debug, Clone, PartialEq)]
pub struct Message {
    pub path: OscPath,
    pub signature: TypeSignature,
    pub args: Vec<OscType>,
    /// The method can be read, so its value may be refreshed after the send.
    pub readable: bool,
}

/// What a recall sends to the tree it is recalled against, and how many captured values match
/// nothing there.
#[derive(Debug, Clone, PartialEq)]
pub struct Recall {
    pub messages: Vec<Message>,
    pub unmatched: usize,
}

/// Matches each captured value with a writable method at the same path with the same type tags.
pub fn recall(preset: &Preset, tree: &TreeState, value_supported: bool) -> Recall {
    let mut messages = Vec::new();
    let mut unmatched = 0;
    for v in &preset.values {
        let found = tree.node(&v.path).and_then(|node| {
            let access = node.effective_access(value_supported);
            let fits = node.is_method()
                && access.writable()
                && node.type_raw.as_deref() == Some(v.type_tags.as_str());
            fits.then_some(())?;
            Some(Message {
                path: node.path.clone()?,
                signature: node.signature.clone()?,
                args: v.args.iter().map(PresetArg::to_osc).collect(),
                readable: access.readable(),
            })
        });
        match found {
            Some(m) => messages.push(m),
            None => unmatched += 1,
        }
    }
    Recall {
        messages,
        unmatched,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use oscq_core::parse_document;

    fn tree(doc: &str) -> TreeState {
        let report = parse_document(doc.as_bytes()).unwrap();
        let mut t = TreeState {
            root: report.root,
            ..TreeState::default()
        };
        t.paths = crate::tree::index_paths(&t.root);
        t
    }

    const DOC: &str = r#"{"FULL_PATH":"/","CONTENTS":{
        "gain":{"FULL_PATH":"/gain","TYPE":"f","ACCESS":3,"VALUE":[0.5]},
        "name":{"FULL_PATH":"/name","TYPE":"s","ACCESS":3,"VALUE":["kick"]},
        "on":{"FULL_PATH":"/on","TYPE":"T","ACCESS":3,"VALUE":[true]},
        "level":{"FULL_PATH":"/level","TYPE":"i","ACCESS":1,"VALUE":[3]},
        "fire":{"FULL_PATH":"/fire","TYPE":"I","ACCESS":2},
        "mode":{"FULL_PATH":"/mode","TYPE":"i","ACCESS":2}}}"#;

    #[test]
    fn capture_keeps_writable_values_and_counts_the_rest() {
        let mut t = tree(DOC);
        t.pending.insert("/mode".into(), vec![OscType::Long(2)]);
        let c = capture("h:1", "H", &t, true, &[]);
        let paths: Vec<&str> = c.preset.values.iter().map(|v| v.path.as_str()).collect();
        assert_eq!(
            paths,
            ["/gain", "/mode", "/name", "/on"],
            "read only /level left out"
        );
        assert_eq!(c.skipped, 1, "/fire, an impulse");
        assert_eq!(
            c.preset.values[1].args,
            [PresetArg::Int(2)],
            "the value sent, not yet read"
        );
        let pinned = vec!["/name".to_owned(), "/gone".to_owned()];
        let c = capture("h:1", "H", &t, true, &pinned);
        assert_eq!(c.preset.values.len(), 1, "only the pinned methods");
    }

    #[test]
    fn recall_sends_what_matches_and_counts_the_rest() {
        let c = capture("h:1", "H", &tree(DOC), true, &[]);
        // /mode, write only and never sent here, has no value: not captured.
        let same = recall(&c.preset, &tree(DOC), true);
        assert_eq!((same.messages.len(), same.unmatched), (3, 0));
        let other = tree(
            r#"{"FULL_PATH":"/","CONTENTS":{
            "gain":{"FULL_PATH":"/gain","TYPE":"f","ACCESS":3},
            "name":{"FULL_PATH":"/name","TYPE":"i","ACCESS":3},
            "on":{"FULL_PATH":"/on","TYPE":"T","ACCESS":1}}}"#,
        );
        let r = recall(&c.preset, &other, true);
        let sent: Vec<&str> = r.messages.iter().map(|m| m.path.as_str()).collect();
        assert_eq!(sent, ["/gain"], "same path, same type, writable");
        assert_eq!(r.unmatched, 2, "/name another type, /on read only");
        assert_eq!(r.messages[0].args, [OscType::Double(0.5)]);
    }
}

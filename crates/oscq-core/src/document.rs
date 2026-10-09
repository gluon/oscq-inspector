//! Address space document parsing. The input is untrusted: every failure is a value, never a panic.

use serde::Deserialize;
use serde_json::{Map, Value};

use crate::model::{ArgEntry, ClipMode, Node, PerArg, Range};
use crate::path::{self, OscPath, PathError};
use crate::types::{TypeError, TypeSignature};

/// Nodes deeper than this keep their `CONTENTS` empty and get a [`DiagnosticKind::DepthLimit`].
pub const MAX_NODE_DEPTH: usize = 64;

/// JSON nesting above this is rejected before parsing, which bounds the parser's stack use.
/// It leaves room for [`MAX_NODE_DEPTH`] nodes, each nesting twice (node object, `CONTENTS`),
/// plus nested per argument arrays.
pub const MAX_JSON_DEPTH: usize = 2 * MAX_NODE_DEPTH + 32;

/// Nested per argument arrays deeper than this are kept as [`ArgEntry::Invalid`].
const MAX_ARG_DEPTH: usize = crate::types::MAX_ARRAY_DEPTH;

#[derive(Debug, thiserror::Error)]
pub enum DocumentError {
    #[error("response is not JSON: {0}")]
    NotJson(#[from] serde_json::Error),
    #[error("JSON is nested deeper than {MAX_JSON_DEPTH} levels")]
    TooDeep,
    #[error("JSON is not an object")]
    NotAnObject,
    #[error("JSON object is not an OSCQuery node: no FULL_PATH, CONTENTS or TYPE")]
    NotAnAddressSpace,
}

/// A problem found in a document that did not prevent parsing it.
#[derive(Debug, Clone, PartialEq)]
pub struct Diagnostic {
    /// Full path of the node concerned, as received or derived.
    pub path: String,
    pub kind: DiagnosticKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DiagnosticKind {
    /// `FULL_PATH` absent or not a string; derived from the parent.
    MissingFullPath,
    /// `FULL_PATH` is not a valid OSC address. The node is shown but cannot be sent to.
    InvalidOscPath(PathError),
    /// `TYPE` could not be parsed. The node is shown but cannot be sent to.
    InvalidType(TypeError),
    /// `VALUE` has a different number of top level entries than `TYPE`.
    ValueLengthMismatch { declared: usize, received: usize },
    /// `ACCESS` is not one of 0, 1, 2, 3. Treated as absent.
    InvalidAccess,
    /// A known attribute with the wrong JSON type. Kept verbatim in `extra`.
    InvalidAttribute(String),
    /// A `CONTENTS` entry that is not an object. Skipped.
    InvalidChild(String),
    /// `CONTENTS` below [`MAX_NODE_DEPTH`] dropped.
    DepthLimit,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParseReport {
    pub root: Node,
    pub diagnostics: Vec<Diagnostic>,
}

/// Parses a JSON value after checking its nesting depth without recursion.
pub(crate) fn parse_json(bytes: &[u8]) -> Result<Value, DocumentError> {
    if json_depth_exceeds(bytes, MAX_JSON_DEPTH) {
        return Err(DocumentError::TooDeep);
    }
    let mut de = serde_json::Deserializer::from_slice(bytes);
    de.disable_recursion_limit();
    let value = Value::deserialize(&mut de)?;
    de.end()?;
    Ok(value)
}

/// True when brackets outside strings nest deeper than `limit`. Malformed input is left to the
/// parser to reject.
fn json_depth_exceeds(bytes: &[u8], limit: usize) -> bool {
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for &b in bytes {
        if in_string {
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if b == b'"' {
                in_string = false;
            }
            continue;
        }
        match b {
            b'"' => in_string = true,
            b'{' | b'[' => {
                depth += 1;
                if depth > limit {
                    return true;
                }
            }
            b'}' | b']' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    false
}

/// Parses an address space document, the reply to a plain GET on a path.
pub fn parse_document(bytes: &[u8]) -> Result<ParseReport, DocumentError> {
    let Value::Object(obj) = parse_json(bytes)? else {
        return Err(DocumentError::NotAnObject);
    };
    if !["FULL_PATH", "CONTENTS", "TYPE"]
        .iter()
        .any(|k| obj.contains_key(*k))
    {
        return Err(DocumentError::NotAnAddressSpace);
    }
    let mut diagnostics = Vec::new();
    let root = node_from_object(obj, "/".to_owned(), 0, true, &mut diagnostics);
    for d in &diagnostics {
        tracing::warn!(path = %d.path, kind = ?d.kind, "address space diagnostic");
    }
    Ok(ParseReport { root, diagnostics })
}

fn node_from_object(
    mut obj: Map<String, Value>,
    derived_path: String,
    depth: usize,
    allow_children: bool,
    diags: &mut Vec<Diagnostic>,
) -> Node {
    let mut node = Node::default();

    match obj.remove("FULL_PATH") {
        Some(Value::String(s)) => node.full_path = s,
        other => {
            node.full_path = derived_path;
            if let Some(v) = other {
                node.extra.insert("FULL_PATH".to_owned(), v);
            }
            // Overloads describe their parent method and carry no FULL_PATH of their own.
            if allow_children {
                diags.push(diag(&node.full_path, DiagnosticKind::MissingFullPath));
            }
        }
    }
    match OscPath::parse(&node.full_path) {
        Ok(p) => node.path = Some(p),
        Err(e) if allow_children => {
            diags.push(diag(&node.full_path, DiagnosticKind::InvalidOscPath(e)))
        }
        Err(_) => {}
    }

    let full_path = node.full_path.clone();
    let bad = |key: &str, v: Value, node: &mut Node, diags: &mut Vec<Diagnostic>| {
        diags.push(diag(
            &full_path,
            DiagnosticKind::InvalidAttribute(key.to_owned()),
        ));
        node.extra.insert(key.to_owned(), v);
    };

    if let Some(v) = obj.remove("TYPE") {
        match v {
            Value::String(s) => {
                match TypeSignature::parse(&s) {
                    Ok(sig) => node.signature = Some(sig),
                    Err(e) => diags.push(diag(&full_path, DiagnosticKind::InvalidType(e))),
                }
                node.type_raw = Some(s);
            }
            other => bad("TYPE", other, &mut node, diags),
        }
    }

    if let Some(v) = obj.remove("ACCESS") {
        match v.as_i64().and_then(crate::model::Access::from_bits) {
            Some(a) => node.access = Some(a),
            None => {
                diags.push(diag(&full_path, DiagnosticKind::InvalidAccess));
                node.extra.insert("ACCESS".to_owned(), v);
            }
        }
    }

    if let Some(v) = obj.remove("DESCRIPTION") {
        match v {
            Value::String(s) => node.description = Some(s),
            other => bad("DESCRIPTION", other, &mut node, diags),
        }
    }

    if let Some(v) = obj.remove("TAGS") {
        match string_array(&v) {
            Some(tags) => node.tags = Some(tags),
            None => bad("TAGS", v, &mut node, diags),
        }
    }

    if let Some(v) = obj.remove("CRITICAL") {
        match v {
            Value::Bool(b) => node.critical = Some(b),
            other => bad("CRITICAL", other, &mut node, diags),
        }
    }

    if let Some(v) = obj.remove("VALUE") {
        node.value = Some(per_arg(v, &value_entry));
    }
    if let Some(v) = obj.remove("RANGE") {
        node.range = Some(per_arg(v, &range_entry));
    }
    if let Some(v) = obj.remove("EXTENDED_TYPE") {
        node.extended_type = Some(per_arg(v, &string_entry));
    }
    if let Some(v) = obj.remove("UNIT") {
        node.unit = Some(per_arg(v, &string_entry));
    }
    if let Some(v) = obj.remove("CLIPMODE") {
        node.clipmode = Some(per_arg(v, &|v| match v {
            Value::String(s) => Ok(ClipMode::parse(&s)),
            other => Err(other),
        }));
    }

    if let (Some(sig), Some(PerArg::List(items))) = (&node.signature, &node.value)
        && items.len() != sig.args().len()
    {
        diags.push(diag(
            &full_path,
            DiagnosticKind::ValueLengthMismatch {
                declared: sig.args().len(),
                received: items.len(),
            },
        ));
    }

    if allow_children {
        if let Some(v) = obj.remove("OVERLOADS") {
            match v {
                Value::Array(items) => {
                    let mut overloads = Vec::with_capacity(items.len());
                    let mut invalid = Vec::new();
                    for item in items {
                        match item {
                            Value::Object(o) => overloads.push(node_from_object(
                                o,
                                full_path.clone(),
                                depth,
                                false,
                                diags,
                            )),
                            other => invalid.push(other),
                        }
                    }
                    if !invalid.is_empty() {
                        bad("OVERLOADS", Value::Array(invalid), &mut node, diags);
                    }
                    node.overloads = Some(overloads);
                }
                other => bad("OVERLOADS", other, &mut node, diags),
            }
        }

        if let Some(v) = obj.remove("CONTENTS") {
            match v {
                Value::Object(children) => {
                    if depth >= MAX_NODE_DEPTH {
                        diags.push(diag(&full_path, DiagnosticKind::DepthLimit));
                        node.contents = Some(Vec::new());
                    } else {
                        let mut out = Vec::with_capacity(children.len());
                        for (name, child) in children {
                            let child_path = path::join(&full_path, &name);
                            match child {
                                Value::Object(o) => {
                                    let n = node_from_object(o, child_path, depth + 1, true, diags);
                                    out.push((name, n));
                                }
                                _ => {
                                    diags.push(diag(&full_path, DiagnosticKind::InvalidChild(name)))
                                }
                            }
                        }
                        node.contents = Some(out);
                    }
                }
                other => bad("CONTENTS", other, &mut node, diags),
            }
        }
    }

    node.extra.extend(obj);
    node
}

fn diag(path: &str, kind: DiagnosticKind) -> Diagnostic {
    Diagnostic {
        path: path.to_owned(),
        kind,
    }
}

fn string_array(v: &Value) -> Option<Vec<String>> {
    v.as_array()?
        .iter()
        .map(|s| s.as_str().map(str::to_owned))
        .collect()
}

type Convert<'a, T> = &'a dyn Fn(Value) -> Result<T, Value>;

/// Builds a per argument attribute from any of the shapes the proposal allows: an array, nested
/// arrays, `null` entries, or a single non array value standing for every entry.
pub(crate) fn per_arg<T>(v: Value, conv: Convert<'_, T>) -> PerArg<T> {
    match v {
        Value::Array(items) => PerArg::List(items.into_iter().map(|i| entry(i, conv, 1)).collect()),
        other => PerArg::Uniform(entry(other, conv, 0)),
    }
}

fn entry<T>(v: Value, conv: Convert<'_, T>, depth: usize) -> ArgEntry<T> {
    match v {
        Value::Null => ArgEntry::Null,
        Value::Array(items) if depth <= MAX_ARG_DEPTH => ArgEntry::Nested(
            items
                .into_iter()
                .map(|i| entry(i, conv, depth + 1))
                .collect(),
        ),
        Value::Array(items) => ArgEntry::Invalid(Value::Array(items)),
        other => match conv(other) {
            Ok(t) => ArgEntry::Value(t),
            Err(raw) => ArgEntry::Invalid(raw),
        },
    }
}

fn value_entry(v: Value) -> Result<Value, Value> {
    match v {
        Value::Object(_) => Err(v),
        other => Ok(other),
    }
}

fn string_entry(v: Value) -> Result<String, Value> {
    match v {
        Value::String(s) => Ok(s),
        other => Err(other),
    }
}

fn range_entry(v: Value) -> Result<Range, Value> {
    let Value::Object(mut o) = v else {
        return Err(v);
    };
    let mut range = Range {
        min: o.remove("MIN"),
        max: o.remove("MAX"),
        ..Range::default()
    };
    match o.remove("VALS") {
        Some(Value::Array(vals)) => range.vals = Some(vals),
        Some(other) => {
            o.insert("VALS".to_owned(), other);
        }
        None => {}
    }
    range.extra = o;
    Ok(range)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn depth_scan_ignores_brackets_in_strings() {
        assert!(!json_depth_exceeds(br#"{"a":"[[[[[[[["}"#, 2));
        assert!(!json_depth_exceeds(br#"{"a":"\"[[[["}"#, 2));
        assert!(json_depth_exceeds(b"[[[", 2));
        assert!(!json_depth_exceeds(b"[[]]", 2));
    }

    #[test]
    fn rejects_non_objects_and_foreign_objects() {
        assert!(matches!(
            parse_document(b"[1,2]"),
            Err(DocumentError::NotAnObject)
        ));
        assert!(matches!(
            parse_document(br#"{"foo":1}"#),
            Err(DocumentError::NotAnAddressSpace)
        ));
        assert!(matches!(
            parse_document(b"<html>"),
            Err(DocumentError::NotJson(_))
        ));
        assert!(matches!(
            parse_document(br#"{"FULL_PATH":"/"} trailing"#),
            Err(DocumentError::NotJson(_))
        ));
    }
}

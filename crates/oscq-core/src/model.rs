//! Typed address space model.

use serde_json::{Map, Value};

use crate::path::OscPath;
use crate::types::TypeSignature;

/// The `ACCESS` bit mask: bit 0 readable, bit 1 writable. 0 means the node has no value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Access(u8);

impl Access {
    pub const NONE: Access = Access(0);
    pub const READ: Access = Access(1);
    pub const WRITE: Access = Access(2);
    pub const READ_WRITE: Access = Access(3);

    /// Accepts the four values the proposal defines, rejects anything else.
    pub fn from_bits(bits: i64) -> Option<Self> {
        match bits {
            0..=3 => u8::try_from(bits).ok().map(Access),
            _ => None,
        }
    }

    pub fn bits(self) -> u8 {
        self.0
    }

    pub fn readable(self) -> bool {
        self.0 & 1 != 0
    }

    pub fn writable(self) -> bool {
        self.0 & 2 != 0
    }
}

/// One entry of a per argument attribute.
#[derive(Debug, Clone, PartialEq)]
pub enum ArgEntry<T> {
    /// JSON `null`: no value for this argument.
    Null,
    Value(T),
    /// A nested array, matching a `[...]` group in `TYPE`.
    Nested(Vec<ArgEntry<T>>),
    /// Present but not of the expected JSON shape. Kept verbatim.
    Invalid(Value),
}

/// A per argument attribute: `VALUE`, `RANGE`, `EXTENDED_TYPE`, `UNIT` or `CLIPMODE`.
#[derive(Debug, Clone, PartialEq)]
pub enum PerArg<T> {
    /// A JSON array, one entry per top level argument of `TYPE`.
    List(Vec<ArgEntry<T>>),
    /// A single non array value standing for every entry.
    Uniform(ArgEntry<T>),
}

impl<T> PerArg<T> {
    /// The entry for top level argument `index`. A uniform value answers for every index.
    pub fn entry(&self, index: usize) -> Option<&ArgEntry<T>> {
        match self {
            PerArg::List(items) => items.get(index),
            PerArg::Uniform(entry) => Some(entry),
        }
    }

    /// Number of top level entries, `None` when uniform.
    pub fn len(&self) -> Option<usize> {
        match self {
            PerArg::List(items) => Some(items.len()),
            PerArg::Uniform(_) => None,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == Some(0)
    }
}

/// One `RANGE` entry.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Range {
    pub min: Option<Value>,
    pub max: Option<Value>,
    pub vals: Option<Vec<Value>>,
    /// Keys other than `MIN`, `MAX`, `VALS`, kept verbatim.
    pub extra: Map<String, Value>,
}

/// One `CLIPMODE` entry. Descriptive only: it says how the server clips, the client never acts on it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClipMode {
    None,
    Low,
    High,
    Both,
    Other(String),
}

impl ClipMode {
    pub fn parse(s: &str) -> Self {
        match s {
            "none" => Self::None,
            "low" => Self::Low,
            "high" => Self::High,
            "both" => Self::Both,
            other => Self::Other(other.to_owned()),
        }
    }
}

/// Colour component names from `EXTENDED_TYPE`. The proposal uses both `color.rgb.*`
/// (`extended_types.md`) and `color.rgba.*` (its README example); both are accepted.
pub fn rgba_color_component(extended_type: &str) -> Option<char> {
    let component = extended_type
        .strip_prefix("color.rgba.")
        .or_else(|| extended_type.strip_prefix("color.rgb."))?;
    let mut chars = component.chars();
    match (chars.next(), chars.next()) {
        (Some(c @ ('r' | 'g' | 'b' | 'a')), None) => Some(c),
        _ => None,
    }
}

/// Parses an `r` value, `#RRGGBBAA`, into its four bytes.
pub fn parse_rgba_hex(s: &str) -> Option<[u8; 4]> {
    let hex = s.strip_prefix('#')?;
    if hex.len() != 8 || !hex.is_ascii() {
        return None;
    }
    let mut out = [0u8; 4];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(hex.get(i * 2..i * 2 + 2)?, 16).ok()?;
    }
    Some(out)
}

/// One node of an address space document.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Node {
    /// `FULL_PATH` as received, or derived from the parent when the server omitted it.
    pub full_path: String,
    /// `full_path` as a valid OSC address, `None` when it is not one. Such a node is shown but
    /// cannot be sent to.
    pub path: Option<OscPath>,
    /// `CONTENTS`, children sorted by name.
    pub contents: Option<Vec<(String, Node)>>,
    /// `TYPE` as received.
    pub type_raw: Option<String>,
    /// `TYPE` parsed, `None` when absent or unparsable.
    pub signature: Option<TypeSignature>,
    pub value: Option<PerArg<Value>>,
    pub range: Option<PerArg<Range>>,
    pub extended_type: Option<PerArg<String>>,
    pub unit: Option<PerArg<String>>,
    pub clipmode: Option<PerArg<ClipMode>>,
    pub access: Option<Access>,
    pub description: Option<String>,
    pub tags: Option<Vec<String>>,
    pub critical: Option<bool>,
    /// `OVERLOADS`, each parsed as a node without `CONTENTS` and without nested overloads.
    pub overloads: Option<Vec<Node>>,
    /// Unknown keys, and known keys whose value had the wrong JSON type, kept verbatim.
    pub extra: Map<String, Value>,
}

impl Node {
    /// A node with `CONTENTS`.
    pub fn is_container(&self) -> bool {
        self.contents.is_some()
    }

    /// A node without `CONTENTS` is a method, even without `TYPE`. A node with both `CONTENTS`
    /// and `TYPE` is a container and a method, as VVOSCQueryProtocol servers publish.
    pub fn is_method(&self) -> bool {
        self.contents.is_none() || self.type_raw.is_some()
    }

    /// The access the client must assume. `ACCESS` when present. When absent, write is allowed,
    /// and read is allowed only if the server lists `VALUE` as supported in `EXTENSIONS`.
    pub fn effective_access(&self, value_supported: bool) -> Access {
        match self.access {
            Some(access) => access,
            None if value_supported => Access::READ_WRITE,
            None => Access::WRITE,
        }
    }

    pub fn children(&self) -> impl Iterator<Item = &Node> {
        self.contents.iter().flatten().map(|(_, node)| node)
    }

    /// Finds a descendant by its full path string.
    pub fn find(&self, full_path: &str) -> Option<&Node> {
        let mut stack = vec![self];
        while let Some(node) = stack.pop() {
            if node.full_path == full_path {
                return Some(node);
            }
            stack.extend(node.children());
        }
        None
    }

    /// Number of nodes in this subtree, this node included.
    pub fn count(&self) -> usize {
        let mut stack = vec![self];
        let mut n = 0;
        while let Some(node) = stack.pop() {
            n += 1;
            stack.extend(node.children());
        }
        n
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn access_bits() {
        assert!(!Access::NONE.readable() && !Access::NONE.writable());
        assert!(Access::READ.readable() && !Access::READ.writable());
        assert!(!Access::WRITE.readable() && Access::WRITE.writable());
        assert!(Access::READ_WRITE.readable() && Access::READ_WRITE.writable());
        assert_eq!(Access::from_bits(4), None);
        assert_eq!(Access::from_bits(-1), None);
    }

    #[test]
    fn absent_access_depends_on_value_extension() {
        let node = Node::default();
        assert_eq!(node.effective_access(true), Access::READ_WRITE);
        assert_eq!(node.effective_access(false), Access::WRITE);
        let node = Node {
            access: Some(Access::READ),
            ..Node::default()
        };
        assert_eq!(node.effective_access(false), Access::READ);
    }

    #[test]
    fn color_spellings() {
        assert_eq!(rgba_color_component("color.rgb.r"), Some('r'));
        assert_eq!(rgba_color_component("color.rgba.a"), Some('a'));
        assert_eq!(rgba_color_component("color.hsv.h"), None);
        assert_eq!(rgba_color_component("color.rgba.rg"), None);
    }

    #[test]
    fn rgba_hex() {
        assert_eq!(parse_rgba_hex("#FA6432FF"), Some([250, 100, 50, 255]));
        assert_eq!(parse_rgba_hex("#0000FF00"), Some([0, 0, 255, 0]));
        assert_eq!(parse_rgba_hex("FA6432FF"), None);
        assert_eq!(parse_rgba_hex("#FA6432"), None);
        assert_eq!(parse_rgba_hex("#\u{e9}\u{e9}\u{e9}\u{e9}"), None);
    }

    #[test]
    fn uniform_answers_every_index() {
        let p: PerArg<String> = PerArg::Uniform(ArgEntry::Value("m".into()));
        assert_eq!(p.entry(7), Some(&ArgEntry::Value("m".into())));
        assert_eq!(p.len(), None);
    }
}

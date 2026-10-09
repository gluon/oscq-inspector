//! OSC type tag strings, as carried by the `TYPE` attribute.

use std::fmt;

/// One OSC type tag, from OSC 1.0 and its common extensions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TypeTag {
    Int32,
    Float32,
    String,
    Blob,
    Int64,
    TimeTag,
    Double,
    Symbol,
    Char,
    Color,
    Midi,
    True,
    False,
    Nil,
    Impulse,
}

impl TypeTag {
    pub fn from_char(c: char) -> Option<Self> {
        Some(match c {
            'i' => Self::Int32,
            'f' => Self::Float32,
            's' => Self::String,
            'b' => Self::Blob,
            'h' => Self::Int64,
            't' => Self::TimeTag,
            'd' => Self::Double,
            'S' => Self::Symbol,
            'c' => Self::Char,
            'r' => Self::Color,
            'm' => Self::Midi,
            'T' => Self::True,
            'F' => Self::False,
            'N' => Self::Nil,
            'I' => Self::Impulse,
            _ => return None,
        })
    }

    pub fn as_char(self) -> char {
        match self {
            Self::Int32 => 'i',
            Self::Float32 => 'f',
            Self::String => 's',
            Self::Blob => 'b',
            Self::Int64 => 'h',
            Self::TimeTag => 't',
            Self::Double => 'd',
            Self::Symbol => 'S',
            Self::Char => 'c',
            Self::Color => 'r',
            Self::Midi => 'm',
            Self::True => 'T',
            Self::False => 'F',
            Self::Nil => 'N',
            Self::Impulse => 'I',
        }
    }

    /// Tags whose value is the tag itself: `T`, `F`, `N`, `I`.
    pub fn carries_no_payload(self) -> bool {
        matches!(self, Self::True | Self::False | Self::Nil | Self::Impulse)
    }
}

/// One argument of a type signature: a single tag, or an array group written `[...]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArgType {
    Tag(TypeTag),
    Array(Vec<ArgType>),
}

/// A parsed `TYPE` attribute. Keeps the string as received.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeSignature {
    raw: String,
    args: Vec<ArgType>,
}

/// Array groups nested deeper than this are rejected, which bounds every recursion over a
/// signature built from untrusted input.
pub const MAX_ARRAY_DEPTH: usize = 16;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TypeError {
    #[error("unknown OSC type tag {0:?}")]
    UnknownTag(char),
    #[error("unbalanced '[' or ']' in type tag string")]
    Unbalanced,
    #[error("array groups nested deeper than {MAX_ARRAY_DEPTH}")]
    TooDeep,
}

impl TypeSignature {
    /// Parses a type tag string. A single leading `,`, as found on the OSC wire, is accepted.
    pub fn parse(raw: &str) -> Result<Self, TypeError> {
        let body = raw.strip_prefix(',').unwrap_or(raw);
        let mut stack: Vec<Vec<ArgType>> = vec![Vec::new()];
        for c in body.chars() {
            match c {
                '[' => {
                    if stack.len() > MAX_ARRAY_DEPTH {
                        return Err(TypeError::TooDeep);
                    }
                    stack.push(Vec::new());
                }
                ']' => {
                    if stack.len() < 2 {
                        return Err(TypeError::Unbalanced);
                    }
                    let group = stack.pop().ok_or(TypeError::Unbalanced)?;
                    stack
                        .last_mut()
                        .ok_or(TypeError::Unbalanced)?
                        .push(ArgType::Array(group));
                }
                _ => {
                    let tag = TypeTag::from_char(c).ok_or(TypeError::UnknownTag(c))?;
                    stack
                        .last_mut()
                        .ok_or(TypeError::Unbalanced)?
                        .push(ArgType::Tag(tag));
                }
            }
        }
        if stack.len() != 1 {
            return Err(TypeError::Unbalanced);
        }
        let args = stack.pop().ok_or(TypeError::Unbalanced)?;
        Ok(Self {
            raw: raw.to_owned(),
            args,
        })
    }

    pub fn as_str(&self) -> &str {
        &self.raw
    }

    /// Top level arguments. Per argument attributes align with this list.
    pub fn args(&self) -> &[ArgType] {
        &self.args
    }

    pub fn is_empty(&self) -> bool {
        self.args.is_empty()
    }
}

impl fmt::Display for TypeSignature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.raw)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use ArgType::{Array, Tag};
    use TypeTag::*;

    #[test]
    fn parses_flat_signatures() {
        let sig = TypeSignature::parse("ffff").unwrap();
        assert_eq!(sig.args(), vec![Tag(Float32); 4].as_slice());
        let sig = TypeSignature::parse("TFNI").unwrap();
        assert_eq!(sig.args(), &[Tag(True), Tag(False), Tag(Nil), Tag(Impulse)]);
        assert!(TypeSignature::parse("").unwrap().is_empty());
    }

    #[test]
    fn parses_array_groups() {
        let sig = TypeSignature::parse("i[ff]i").unwrap();
        assert_eq!(
            sig.args(),
            &[
                Tag(Int32),
                Array(vec![Tag(Float32), Tag(Float32)]),
                Tag(Int32)
            ]
        );
        let sig = TypeSignature::parse("[[s]]").unwrap();
        assert_eq!(sig.args(), &[Array(vec![Array(vec![Tag(String)])])]);
    }

    #[test]
    fn accepts_wire_comma_prefix() {
        let sig = TypeSignature::parse(",if").unwrap();
        assert_eq!(sig.args(), &[Tag(Int32), Tag(Float32)]);
        assert_eq!(sig.as_str(), ",if");
    }

    #[test]
    fn rejects_bad_signatures() {
        assert_eq!(TypeSignature::parse("fx"), Err(TypeError::UnknownTag('x')));
        assert_eq!(TypeSignature::parse("[f"), Err(TypeError::Unbalanced));
        assert_eq!(TypeSignature::parse("f]"), Err(TypeError::Unbalanced));
        let deep = format!("{}f{}", "[".repeat(40), "]".repeat(40));
        assert_eq!(TypeSignature::parse(&deep), Err(TypeError::TooDeep));
        let ok = format!(
            "{}f{}",
            "[".repeat(MAX_ARRAY_DEPTH),
            "]".repeat(MAX_ARRAY_DEPTH)
        );
        assert!(TypeSignature::parse(&ok).is_ok());
    }
}

//! OSC address validation.

use std::fmt;

/// A validated OSC address, as defined by OSC 1.0: a `/` followed by parts made of printable
/// ASCII, excluding the characters OSC reserves for pattern matching and separation.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct OscPath(String);

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PathError {
    #[error("OSC address is empty")]
    Empty,
    #[error("OSC address must start with '/'")]
    MissingLeadingSlash,
    #[error("OSC address contains an empty part")]
    EmptyPart,
    #[error("OSC address contains {0:?}, which OSC does not allow in an address")]
    ForbiddenChar(char),
}

/// Characters OSC 1.0 forbids inside an address part, besides non printable characters.
const RESERVED: &[char] = &[' ', '#', '*', ',', '/', '?', '[', ']', '{', '}'];

impl OscPath {
    /// The root container, `/`.
    pub fn root() -> Self {
        OscPath("/".to_owned())
    }

    pub fn parse(s: &str) -> Result<Self, PathError> {
        if s.is_empty() {
            return Err(PathError::Empty);
        }
        if s == "/" {
            return Ok(Self::root());
        }
        let Some(rest) = s.strip_prefix('/') else {
            return Err(PathError::MissingLeadingSlash);
        };
        for part in rest.split('/') {
            if part.is_empty() {
                return Err(PathError::EmptyPart);
            }
            if let Some(c) = part
                .chars()
                .find(|c| !c.is_ascii_graphic() || RESERVED.contains(c))
            {
                return Err(PathError::ForbiddenChar(c));
            }
        }
        Ok(OscPath(s.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn is_root(&self) -> bool {
        self.0 == "/"
    }

    /// Address parts, empty for the root.
    pub fn segments(&self) -> impl Iterator<Item = &str> {
        self.0.split('/').filter(|p| !p.is_empty())
    }
}

impl fmt::Display for OscPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Joins a child name to a parent path string without validating either.
pub(crate) fn join(parent: &str, child: &str) -> String {
    if parent.ends_with('/') {
        format!("{parent}{child}")
    } else {
        format!("{parent}/{child}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_valid_addresses() {
        for s in ["/", "/foo", "/foo/bar_1", "/a.b-c/~x", "/layer1/opacity"] {
            assert_eq!(OscPath::parse(s).unwrap().as_str(), s);
        }
    }

    #[test]
    fn rejects_characters_legal_in_json_but_not_in_osc() {
        assert_eq!(OscPath::parse("/a b"), Err(PathError::ForbiddenChar(' ')));
        assert_eq!(OscPath::parse("/a#b"), Err(PathError::ForbiddenChar('#')));
        assert_eq!(OscPath::parse("/a*"), Err(PathError::ForbiddenChar('*')));
        assert_eq!(OscPath::parse("/a{b}"), Err(PathError::ForbiddenChar('{')));
        assert_eq!(
            OscPath::parse("/a\u{1}"),
            Err(PathError::ForbiddenChar('\u{1}'))
        );
        assert_eq!(
            OscPath::parse("/caf\u{e9}"),
            Err(PathError::ForbiddenChar('\u{e9}'))
        );
    }

    #[test]
    fn rejects_malformed_addresses() {
        assert_eq!(OscPath::parse(""), Err(PathError::Empty));
        assert_eq!(OscPath::parse("foo"), Err(PathError::MissingLeadingSlash));
        assert_eq!(OscPath::parse("/foo//bar"), Err(PathError::EmptyPart));
        assert_eq!(OscPath::parse("/foo/"), Err(PathError::EmptyPart));
    }

    #[test]
    fn join_handles_root() {
        assert_eq!(join("/", "a"), "/a");
        assert_eq!(join("/a", "b"), "/a/b");
    }
}

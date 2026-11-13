//! Raw JSON for the JSON tab: the exact text that arrived for a subtree, indented for reading
//! without changing a single token.

use std::collections::BTreeMap;

use serde_json::value::RawValue;

/// The exact text of the node reached by following `names` through `CONTENTS`, as a slice of
/// `raw`. `None` when the chain is not in the document.
pub fn raw_subtree<'a>(raw: &'a str, names: &[String]) -> Option<&'a str> {
    let mut current: &'a RawValue = serde_json::from_str(raw).ok()?;
    for name in names {
        let node: BTreeMap<String, &'a RawValue> = serde_json::from_str(current.get()).ok()?;
        let contents: BTreeMap<String, &'a RawValue> =
            serde_json::from_str(node.get("CONTENTS")?.get()).ok()?;
        current = contents.get(name.as_str())?;
    }
    Some(current.get())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Token {
    Key,
    Str,
    Number,
    Literal,
    Punct,
}

/// One displayed line: its indent level and its tokens, each kept byte for byte.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub indent: usize,
    pub tokens: Vec<(Token, String)>,
}

fn lex(raw: &str) -> Vec<(Token, &str)> {
    let bytes = raw.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        match b {
            b' ' | b'\t' | b'\n' | b'\r' => i += 1,
            b'{' | b'}' | b'[' | b']' | b':' | b',' => {
                out.push((Token::Punct, &raw[i..i + 1]));
                i += 1;
            }
            b'"' => {
                let start = i;
                i += 1;
                while i < bytes.len() {
                    match bytes[i] {
                        b'\\' => i += 2,
                        b'"' => {
                            i += 1;
                            break;
                        }
                        _ => i += 1,
                    }
                }
                let end = i.min(bytes.len());
                out.push((Token::Str, &raw[start..end]));
            }
            _ => {
                let start = i;
                while i < bytes.len()
                    && !matches!(
                        bytes[i],
                        b' ' | b'\t'
                            | b'\n'
                            | b'\r'
                            | b'{'
                            | b'}'
                            | b'['
                            | b']'
                            | b':'
                            | b','
                            | b'"'
                    )
                {
                    i += 1;
                }
                let text = &raw[start..i];
                let kind = if matches!(text, "true" | "false" | "null") {
                    Token::Literal
                } else {
                    Token::Number
                };
                out.push((kind, text));
            }
        }
    }
    // A string followed by a colon is a key.
    for k in 0..out.len() {
        if out[k].0 == Token::Str && out.get(k + 1).is_some_and(|t| t.1 == ":") {
            out[k].0 = Token::Key;
        }
    }
    out
}

/// Re-indents `raw` two spaces per level. Only whitespace between tokens changes: the tokens,
/// their order and duplicates are exactly those received.
pub fn pretty(raw: &str) -> Vec<Line> {
    let tokens = lex(raw);
    let mut lines = Vec::new();
    let mut line = Line {
        indent: 0,
        tokens: Vec::new(),
    };
    let mut depth = 0usize;
    let mut k = 0;
    let flush = |line: &mut Line, lines: &mut Vec<Line>, next_indent: usize| {
        if !line.tokens.is_empty() {
            lines.push(std::mem::replace(
                line,
                Line {
                    indent: next_indent,
                    tokens: Vec::new(),
                },
            ));
        } else {
            line.indent = next_indent;
        }
    };
    while k < tokens.len() {
        let (kind, text) = tokens[k];
        match text {
            "{" | "[" => {
                let close = if text == "{" { "}" } else { "]" };
                if tokens.get(k + 1).is_some_and(|t| t.1 == close) {
                    line.tokens.push((kind, text.to_owned()));
                    line.tokens.push((Token::Punct, close.to_owned()));
                    k += 2;
                    continue;
                }
                line.tokens.push((kind, text.to_owned()));
                depth += 1;
                flush(&mut line, &mut lines, depth);
            }
            "}" | "]" if kind == Token::Punct => {
                depth = depth.saturating_sub(1);
                flush(&mut line, &mut lines, depth);
                line.tokens.push((kind, text.to_owned()));
            }
            "," if kind == Token::Punct => {
                line.tokens.push((kind, text.to_owned()));
                flush(&mut line, &mut lines, depth);
            }
            ":" if kind == Token::Punct => line.tokens.push((kind, ": ".to_owned())),
            _ => line.tokens.push((kind, text.to_owned())),
        }
        k += 1;
    }
    flush(&mut line, &mut lines, 0);
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = r#"{"FULL_PATH":"/","CONTENTS":{"a b":{"FULL_PATH":"/a b","VALUE":[1.0,  -2e3],"X":{},"CONTENTS":{"c":{"FULL_PATH":"/a b/c","TYPE":"s","VALUE":["q\"}{]"]}}}},"E":[]}"#;

    fn squash(s: &str) -> String {
        lex(s).into_iter().map(|t| t.1).collect()
    }

    #[test]
    fn subtree_is_the_exact_received_slice() {
        assert_eq!(raw_subtree(DOC, &[]), Some(DOC));
        let sub = raw_subtree(DOC, &["a b".into()]).unwrap();
        assert!(DOC.contains(sub));
        assert!(sub.starts_with(r#"{"FULL_PATH":"/a b","VALUE":[1.0,  -2e3]"#));
        let leaf = raw_subtree(DOC, &["a b".into(), "c".into()]).unwrap();
        assert_eq!(
            leaf,
            r#"{"FULL_PATH":"/a b/c","TYPE":"s","VALUE":["q\"}{]"]}"#
        );
        assert_eq!(raw_subtree(DOC, &["missing".into()]), None);
    }

    #[test]
    fn pretty_changes_whitespace_only() {
        let lines = pretty(DOC);
        let joined: String = lines
            .iter()
            .flat_map(|l| l.tokens.iter().map(|t| t.1.trim_end()))
            .collect();
        assert_eq!(squash(&joined), squash(DOC));
        let all: Vec<&str> = lines
            .iter()
            .flat_map(|l| l.tokens.iter().map(|t| t.1.as_str()))
            .collect();
        assert!(all.contains(&"1.0"), "number text kept as received");
        assert!(all.contains(&"-2e3"));
        assert!(
            all.contains(&r#""q\"}{]""#),
            "brackets inside strings are not structure"
        );
    }

    #[test]
    fn pretty_layout() {
        let lines = pretty(r#"{"a":[1,2],"b":{}}"#);
        let text: Vec<String> = lines
            .iter()
            .map(|l| {
                format!(
                    "{}{}",
                    "  ".repeat(l.indent),
                    l.tokens.iter().map(|t| t.1.as_str()).collect::<String>()
                )
            })
            .collect();
        assert_eq!(
            text,
            [
                "{",
                "  \"a\": [",
                "    1,",
                "    2",
                "  ],",
                "  \"b\": {}",
                "}"
            ]
        );
        assert_eq!(lines[1].tokens[0].0, Token::Key);
        assert_eq!(lines[2].tokens[0].0, Token::Number);
    }
}

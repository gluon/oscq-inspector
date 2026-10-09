//! Which editing widget a node gets (PRD 9.3), and whether it may show a widget or a value at all.

use oscq_core::model::rgba_color_component;
use oscq_core::{Access, ArgEntry, ArgType, Node, PerArg, Range, TypeTag};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
pub enum WidgetKind {
    /// `f` or `d` with MIN and MAX: slider plus numeric field.
    FloatSlider { min: f64, max: f64 },
    /// `f` or `d` without range: drag value, fine with shift held.
    FloatDrag,
    /// `i` or `h` with MIN and MAX.
    IntSlider { min: i64, max: i64 },
    /// `i` or `s` with VALS.
    Dropdown { vals: Vec<Value> },
    /// `s` free: single line, commit on enter.
    Text,
    /// `T` or `F`.
    Toggle,
    /// `I` or `N`: momentary button.
    Momentary,
    /// `r` without any EXTENDED_TYPE.
    ColourR,
    /// `ffff` named as a colour by EXTENDED_TYPE.
    ColourFloats,
    /// `ff` or `fff`: grouped drag values, each clamped to its RANGE when it has one.
    Grouped { ranges: Vec<Option<(f64, f64)>> },
    /// Anything else: read only display of the value plus a raw argument entry field.
    Raw,
}

fn single_tag(node: &Node) -> Option<TypeTag> {
    match node.signature.as_ref()?.args() {
        [ArgType::Tag(t)] => Some(*t),
        _ => None,
    }
}

fn flat_tags(node: &Node) -> Option<Vec<TypeTag>> {
    node.signature
        .as_ref()?
        .args()
        .iter()
        .map(|a| match a {
            ArgType::Tag(t) => Some(*t),
            ArgType::Array(_) => None,
        })
        .collect()
}

pub fn range_at(node: &Node, i: usize) -> Option<&Range> {
    match node.range.as_ref()?.entry(i)? {
        ArgEntry::Value(r) => Some(r),
        _ => None,
    }
}

pub fn min_max(range: Option<&Range>) -> Option<(f64, f64)> {
    let r = range?;
    let min = r.min.as_ref()?.as_f64()?;
    let max = r.max.as_ref()?.as_f64()?;
    (min.is_finite() && max.is_finite() && min < max).then_some((min, max))
}

fn vals(range: Option<&Range>) -> Option<Vec<Value>> {
    range?.vals.clone().filter(|v| !v.is_empty())
}

fn is_rgba_extended_type(node: &Node) -> bool {
    let Some(et) = node.extended_type.as_ref() else {
        return false;
    };
    let comps: Option<Vec<char>> = (0..4)
        .map(|i| match et.entry(i)? {
            ArgEntry::Value(s) => rgba_color_component(s),
            _ => None,
        })
        .collect();
    matches!(comps.as_deref(), Some(['r', 'g', 'b', 'a']))
}

/// The table of PRD 9.3, first matching row wins.
pub fn choose(node: &Node) -> WidgetKind {
    use TypeTag::*;
    if let Some(tag) = single_tag(node) {
        let r = range_at(node, 0);
        match tag {
            Float32 | Double => {
                return match min_max(r) {
                    Some((min, max)) => WidgetKind::FloatSlider { min, max },
                    None => WidgetKind::FloatDrag,
                };
            }
            Int32 | Int64 => {
                if let Some((min, max)) = min_max(r) {
                    return WidgetKind::IntSlider {
                        min: min.ceil() as i64,
                        max: max.floor() as i64,
                    };
                }
                if tag == Int32
                    && let Some(vals) = vals(r)
                {
                    return WidgetKind::Dropdown { vals };
                }
                return WidgetKind::Raw;
            }
            String => {
                return match vals(r) {
                    Some(vals) => WidgetKind::Dropdown { vals },
                    None => WidgetKind::Text,
                };
            }
            True | False => return WidgetKind::Toggle,
            Impulse | Nil => return WidgetKind::Momentary,
            Color if node.extended_type.is_none() => return WidgetKind::ColourR,
            _ => return WidgetKind::Raw,
        }
    }
    let tags = flat_tags(node).unwrap_or_default();
    let all_float = !tags.is_empty() && tags.iter().all(|t| *t == TypeTag::Float32);
    match tags.len() {
        4 if all_float && is_rgba_extended_type(node) => WidgetKind::ColourFloats,
        2 | 3 if all_float => WidgetKind::Grouped {
            ranges: (0..tags.len())
                .map(|i| min_max(range_at(node, i)))
                .collect(),
        },
        _ => WidgetKind::Raw,
    }
}

/// What a row may show. A widget only when the node is writable and a send can succeed; a value
/// only when it is readable. Never an editable widget that cannot send.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Presentation {
    pub widget: bool,
    pub value: bool,
    /// Why there is no widget on a writable node.
    pub blocked: Option<String>,
}

pub fn present(node: &Node, access: Access, send_blocked: Option<&str>) -> Presentation {
    let mut blocked = None;
    let mut widget = access.writable() && node.is_method();
    if widget {
        let reason = if let Some(r) = send_blocked {
            Some(r.to_owned())
        } else if node.path.is_none() {
            Some("not a valid OSC address".to_owned())
        } else if node.type_raw.is_some() && node.signature.is_none() {
            Some("TYPE could not be parsed".to_owned())
        } else if node.type_raw.as_deref().is_some_and(|t| t.contains('S')) {
            Some("type S cannot be sent".to_owned())
        } else {
            None
        };
        if reason.is_some() {
            widget = false;
            blocked = reason;
        }
    }
    Presentation {
        widget,
        value: access.readable(),
        blocked,
    }
}

/// A short text for a value, as shown in the value column.
pub fn value_text(value: &PerArg<Value>) -> String {
    fn entry(e: &ArgEntry<Value>) -> String {
        match e {
            ArgEntry::Null => "null".to_owned(),
            ArgEntry::Value(Value::Number(n)) => match n.as_f64() {
                Some(f) if n.is_f64() => format!("{f:.3}"),
                _ => n.to_string(),
            },
            ArgEntry::Value(v) | ArgEntry::Invalid(v) => v.to_string(),
            ArgEntry::Nested(items) => {
                format!(
                    "[{}]",
                    items.iter().map(entry).collect::<Vec<_>>().join(" ")
                )
            }
        }
    }
    match value {
        PerArg::List(items) => items.iter().map(entry).collect::<Vec<_>>().join(" "),
        PerArg::Uniform(e) => entry(e),
    }
}

pub fn value_f64(value: Option<&PerArg<Value>>, i: usize) -> Option<f64> {
    match value?.entry(i)? {
        ArgEntry::Value(v) => v.as_f64(),
        _ => None,
    }
}

pub fn value_str(value: Option<&PerArg<Value>>, i: usize) -> Option<String> {
    match value?.entry(i)? {
        ArgEntry::Value(Value::String(s)) => Some(s.clone()),
        _ => None,
    }
}

pub fn value_bool(value: Option<&PerArg<Value>>, i: usize) -> Option<bool> {
    match value?.entry(i)? {
        ArgEntry::Value(Value::Bool(b)) => Some(*b),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oscq_core::parse_document;
    use serde_json::json;

    fn node(v: Value) -> Node {
        let mut v = v;
        v["FULL_PATH"] = json!("/n");
        parse_document(v.to_string().as_bytes()).unwrap().root
    }

    #[test]
    fn the_table_of_prd_9_3() {
        let r01 = json!([{"MIN": 0.0, "MAX": 1.0}]);
        let cases = [
            (
                json!({"TYPE":"f","RANGE":r01}),
                WidgetKind::FloatSlider { min: 0.0, max: 1.0 },
            ),
            (
                json!({"TYPE":"d","RANGE":r01}),
                WidgetKind::FloatSlider { min: 0.0, max: 1.0 },
            ),
            (json!({"TYPE":"f"}), WidgetKind::FloatDrag),
            (
                json!({"TYPE":"d","RANGE":[{"MIN":0}]}),
                WidgetKind::FloatDrag,
            ),
            (
                json!({"TYPE":"i","RANGE":[{"MIN":0,"MAX":100}]}),
                WidgetKind::IntSlider { min: 0, max: 100 },
            ),
            (
                json!({"TYPE":"h","RANGE":[{"MIN":0,"MAX":100}]}),
                WidgetKind::IntSlider { min: 0, max: 100 },
            ),
            (
                json!({"TYPE":"i","RANGE":[{"VALS":[1,2,3]}]}),
                WidgetKind::Dropdown {
                    vals: vec![json!(1), json!(2), json!(3)],
                },
            ),
            (
                json!({"TYPE":"s","RANGE":[{"VALS":["a","b"]}]}),
                WidgetKind::Dropdown {
                    vals: vec![json!("a"), json!("b")],
                },
            ),
            (json!({"TYPE":"s"}), WidgetKind::Text),
            (json!({"TYPE":"T"}), WidgetKind::Toggle),
            (json!({"TYPE":"F"}), WidgetKind::Toggle),
            (json!({"TYPE":"I"}), WidgetKind::Momentary),
            (json!({"TYPE":"N"}), WidgetKind::Momentary),
            (json!({"TYPE":"r"}), WidgetKind::ColourR),
            (
                json!({"TYPE":"ffff","EXTENDED_TYPE":["color.rgb.r","color.rgb.g","color.rgb.b","color.rgb.a"]}),
                WidgetKind::ColourFloats,
            ),
            (
                json!({"TYPE":"ffff","EXTENDED_TYPE":["color.rgba.r","color.rgba.g","color.rgba.b","color.rgba.a"]}),
                WidgetKind::ColourFloats,
            ),
            (
                json!({"TYPE":"ff","RANGE":[{"MIN":0.0,"MAX":1.0}, null]}),
                WidgetKind::Grouped {
                    ranges: vec![Some((0.0, 1.0)), None],
                },
            ),
            (
                json!({"TYPE":"fff"}),
                WidgetKind::Grouped {
                    ranges: vec![None, None, None],
                },
            ),
            (json!({"TYPE":"ffff"}), WidgetKind::Raw),
            (json!({"TYPE":"i"}), WidgetKind::Raw),
            (json!({"TYPE":"h","RANGE":[{"VALS":[1]}]}), WidgetKind::Raw),
            (
                json!({"TYPE":"r","EXTENDED_TYPE":["color.rgba"]}),
                WidgetKind::Raw,
            ),
            (json!({"TYPE":"iiii"}), WidgetKind::Raw),
            (json!({"TYPE":"i[ii]i"}), WidgetKind::Raw),
            (json!({"TYPE":"m"}), WidgetKind::Raw),
            (json!({"DESCRIPTION":"no TYPE"}), WidgetKind::Raw),
        ];
        for (doc, want) in cases {
            assert_eq!(choose(&node(doc.clone())), want, "{doc}");
        }
    }

    #[test]
    fn uniform_range_applies_to_the_single_argument() {
        let n = node(json!({"TYPE":"f","RANGE":{"MIN":-1.0,"MAX":1.0}}));
        assert_eq!(
            choose(&n),
            WidgetKind::FloatSlider {
                min: -1.0,
                max: 1.0
            }
        );
    }

    #[test]
    fn presentation_rules() {
        let n = node(json!({"TYPE":"f"}));
        assert_eq!(
            present(&n, Access::WRITE, None),
            Presentation {
                widget: true,
                value: false,
                blocked: None
            }
        );
        assert_eq!(
            present(&n, Access::READ, None),
            Presentation {
                widget: false,
                value: true,
                blocked: None
            }
        );
        let tcp = present(&n, Access::READ_WRITE, Some("TCP"));
        assert!(!tcp.widget && tcp.value);
        assert_eq!(tcp.blocked.as_deref(), Some("TCP"));
        let none = present(&n, Access::NONE, None);
        assert!(!none.widget && !none.value);
        let bad = node(json!({"TYPE":"fx"}));
        assert!(!present(&bad, Access::WRITE, None).widget);
        let symbol = node(json!({"TYPE":"S"}));
        assert!(!present(&symbol, Access::WRITE, None).widget);
        let container = node(json!({"CONTENTS":{}}));
        assert!(!present(&container, Access::READ_WRITE, None).widget);
    }

    #[test]
    fn value_texts() {
        let v = oscq_core::document::parse_value(json!([0.5, 3, "x", true, null, [1, 2]]));
        assert_eq!(value_text(&v), "0.500 3 \"x\" true null [1 2]");
        assert_eq!(value_f64(Some(&v), 1), Some(3.0));
        assert_eq!(value_str(Some(&v), 2).as_deref(), Some("x"));
        assert_eq!(value_bool(Some(&v), 3), Some(true));
    }
}

//! Tree model for the virtualised view: visible rows, rerooting by path prefix, fuzzy filter.

use std::collections::{HashMap, HashSet};

use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};
use oscq_core::Node;

/// A node addressed by the child indices leading to it from the tree root.
pub type NodeIndex = Vec<u32>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub index: NodeIndex,
    pub depth: usize,
}

pub fn node_at<'a>(root: &'a Node, index: &[u32]) -> Option<&'a Node> {
    let mut node = root;
    for &i in index {
        node = &node.contents.as_ref()?.get(i as usize)?.1;
    }
    Some(node)
}

pub fn node_at_mut<'a>(root: &'a mut Node, index: &[u32]) -> Option<&'a mut Node> {
    let mut node = root;
    for &i in index {
        node = &mut node.contents.as_mut()?.get_mut(i as usize)?.1;
    }
    Some(node)
}

/// Child names from the root to a node, the key chain of the raw document.
pub fn names_at(root: &Node, index: &[u32]) -> Option<Vec<String>> {
    let mut node = root;
    let mut names = Vec::with_capacity(index.len());
    for &i in index {
        let (name, child) = node.contents.as_ref()?.get(i as usize)?;
        names.push(name.clone());
        node = child;
    }
    Some(names)
}

/// The index of the node with a given full path, without recursion.
pub fn find(root: &Node, full_path: &str) -> Option<NodeIndex> {
    let mut stack: Vec<(&Node, NodeIndex)> = vec![(root, Vec::new())];
    while let Some((node, index)) = stack.pop() {
        if node.full_path == full_path {
            return Some(index);
        }
        for (i, (_, child)) in node.contents.iter().flatten().enumerate() {
            let mut ci = index.clone();
            ci.push(i as u32);
            stack.push((child, ci));
        }
    }
    None
}

/// Every node's index by full path, without recursion.
pub fn index_paths(root: &Node) -> HashMap<String, NodeIndex> {
    let mut out = HashMap::new();
    let mut stack: Vec<(&Node, NodeIndex)> = vec![(root, Vec::new())];
    while let Some((node, index)) = stack.pop() {
        for (i, (_, child)) in node.contents.iter().flatten().enumerate() {
            let mut ci = index.clone();
            ci.push(i as u32);
            stack.push((child, ci));
        }
        out.insert(node.full_path.clone(), index);
    }
    out
}

/// Rows shown under `base`, children first level at depth 0, descending only into expanded
/// containers. Built iteratively so the depth of the tree costs no stack.
pub fn visible_rows(root: &Node, base: &[u32], expanded: &HashSet<String>) -> Vec<Row> {
    let mut rows = Vec::new();
    let Some(start) = node_at(root, base) else {
        return rows;
    };
    let mut stack: Vec<(&Node, NodeIndex, usize)> = Vec::new();
    fn push_children<'a>(
        stack: &mut Vec<(&'a Node, NodeIndex, usize)>,
        node: &'a Node,
        index: &NodeIndex,
        depth: usize,
    ) {
        if let Some(children) = node.contents.as_ref() {
            for (i, (_, child)) in children.iter().enumerate().rev() {
                let mut ci = index.clone();
                ci.push(i as u32);
                stack.push((child, ci, depth));
            }
        }
    }
    push_children(&mut stack, start, &base.to_vec(), 0);
    while let Some((node, index, depth)) = stack.pop() {
        let open = node.is_container() && expanded.contains(&node.full_path);
        rows.push(Row {
            index: index.clone(),
            depth,
        });
        if open {
            push_children(&mut stack, node, &index, depth + 1);
        }
    }
    rows
}

/// One fuzzy match: the node, and the char positions of its full path that matched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    pub index: NodeIndex,
    pub score: u32,
    pub positions: Vec<u32>,
}

/// Every node under the root whose full path matches `query`, best first. Purely local: never
/// triggers a request.
pub fn fuzzy(root: &Node, query: &str) -> Vec<Hit> {
    let query = query.trim();
    if query.is_empty() {
        return Vec::new();
    }
    let pattern = Pattern::parse(query, CaseMatching::Smart, Normalization::Smart);
    let mut matcher = Matcher::new(Config::DEFAULT.match_paths());
    let mut buf = Vec::new();
    let mut hits = Vec::new();
    let mut stack: Vec<(&Node, NodeIndex)> = vec![(root, Vec::new())];
    while let Some((node, index)) = stack.pop() {
        if !index.is_empty() {
            let mut positions = Vec::new();
            if let Some(score) = pattern.indices(
                Utf32Str::new(&node.full_path, &mut buf),
                &mut matcher,
                &mut positions,
            ) {
                positions.sort_unstable();
                positions.dedup();
                hits.push(Hit {
                    index: index.clone(),
                    score,
                    positions,
                });
            }
        }
        for (i, (_, child)) in node.contents.iter().flatten().enumerate() {
            let mut ci = index.clone();
            ci.push(i as u32);
            stack.push((child, ci));
        }
    }
    hits.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.index.cmp(&b.index)));
    hits
}

#[cfg(test)]
mod tests {
    use super::*;
    use oscq_core::parse_document;

    fn tree() -> Node {
        parse_document(
            br#"{"FULL_PATH":"/","CONTENTS":{
                "layer1":{"FULL_PATH":"/layer1","CONTENTS":{
                    "opacity":{"FULL_PATH":"/layer1/opacity","TYPE":"f"},
                    "fx":{"FULL_PATH":"/layer1/fx","CONTENTS":{
                        "blur":{"FULL_PATH":"/layer1/fx/blur","TYPE":"f"}}}}},
                "master":{"FULL_PATH":"/master","TYPE":"f"}}}"#,
        )
        .unwrap()
        .root
    }

    fn paths(root: &Node, rows: &[Row]) -> Vec<String> {
        rows.iter()
            .map(|r| node_at(root, &r.index).unwrap().full_path.clone())
            .collect()
    }

    #[test]
    fn collapsed_shows_first_level_only() {
        let t = tree();
        let rows = visible_rows(&t, &[], &HashSet::new());
        assert_eq!(paths(&t, &rows), ["/layer1", "/master"]);
        assert!(rows.iter().all(|r| r.depth == 0));
    }

    #[test]
    fn expanded_containers_show_children_in_order() {
        let t = tree();
        let expanded: HashSet<String> = ["/layer1".to_owned(), "/layer1/fx".to_owned()].into();
        let rows = visible_rows(&t, &[], &expanded);
        assert_eq!(
            paths(&t, &rows),
            [
                "/layer1",
                "/layer1/fx",
                "/layer1/fx/blur",
                "/layer1/opacity",
                "/master"
            ]
        );
        let depths: Vec<usize> = rows.iter().map(|r| r.depth).collect();
        assert_eq!(depths, [0, 1, 2, 1, 0]);
    }

    #[test]
    fn reroot_at_a_container() {
        let t = tree();
        let base = find(&t, "/layer1").unwrap();
        let rows = visible_rows(&t, &base, &HashSet::new());
        assert_eq!(paths(&t, &rows), ["/layer1/fx", "/layer1/opacity"]);
        assert_eq!(names_at(&t, &base).unwrap(), ["layer1"]);
    }

    #[test]
    fn find_and_index_round_trip() {
        let t = tree();
        let i = find(&t, "/layer1/fx/blur").unwrap();
        assert_eq!(node_at(&t, &i).unwrap().full_path, "/layer1/fx/blur");
        assert_eq!(names_at(&t, &i).unwrap(), ["layer1", "fx", "blur"]);
        assert_eq!(find(&t, "/nope"), None);
    }

    #[test]
    fn fuzzy_matches_full_paths_with_positions() {
        let t = tree();
        let hits = fuzzy(&t, "blr");
        assert_eq!(hits.len(), 1);
        let n = node_at(&t, &hits[0].index).unwrap();
        assert_eq!(n.full_path, "/layer1/fx/blur");
        let chars: String = hits[0]
            .positions
            .iter()
            .map(|&p| n.full_path.chars().nth(p as usize).unwrap())
            .collect();
        assert_eq!(chars, "blr");
        assert!(fuzzy(&t, "  ").is_empty());
        assert!(fuzzy(&t, "zzz").is_empty());
    }

    #[test]
    fn five_thousand_children_expand_without_stack() {
        let mut doc =
            String::from(r#"{"FULL_PATH":"/","CONTENTS":{"big":{"FULL_PATH":"/big","CONTENTS":{"#);
        let items: Vec<String> = (0..5000)
            .map(|i| format!(r#""p{i}":{{"FULL_PATH":"/big/p{i}","TYPE":"f"}}"#))
            .collect();
        doc.push_str(&items.join(","));
        doc.push_str("}}}}");
        let t = parse_document(doc.as_bytes()).unwrap().root;
        let expanded: HashSet<String> = ["/big".to_owned()].into();
        assert_eq!(visible_rows(&t, &[], &expanded).len(), 5001);
    }
}

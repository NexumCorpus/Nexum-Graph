#![no_main]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use nex_core::{ChangeKind, DepKind, SemanticId, SemanticUnit, UnitKind};
use nex_graph::CodeGraph;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

#[derive(Arbitrary, Clone, Debug)]
struct GraphInput {
    before: Vec<ArbUnit>,
    after: Vec<ArbUnit>,
    before_edges: Vec<ArbEdge>,
    after_edges: Vec<ArbEdge>,
}

#[derive(Arbitrary, Clone, Debug)]
struct ArbUnit {
    name: String,
    file: String,
    signature_hash: u64,
    body_hash: u64,
    kind: ArbUnitKind,
}

#[derive(Arbitrary, Clone, Debug)]
struct ArbEdge {
    from: u8,
    to: u8,
    kind: ArbDepKind,
}

#[derive(Arbitrary, Clone, Copy, Debug)]
enum ArbUnitKind {
    Function,
    Method,
    Class,
    Struct,
    Interface,
    Trait,
    Enum,
    Module,
    Constant,
}

#[derive(Arbitrary, Clone, Copy, Debug)]
enum ArbDepKind {
    Calls,
    Imports,
    Inherits,
    Implements,
    Uses,
}

fuzz_target!(|input: GraphInput| {
    let before_units = normalize_units("before", &input.before);
    let after_units = normalize_units("after", &input.after);

    let before_graph = build_graph(&before_units, &input.before_edges);
    let after_graph = build_graph(&after_units, &input.after_edges);
    let before_edge_free = build_graph(&before_units, &[]);
    let after_edge_free = build_graph(&after_units, &[]);

    let diff = before_graph.diff(&after_graph);
    let edge_free_diff = before_edge_free.diff(&after_edge_free);

    assert_eq!(
        classification_summary(&diff),
        classification_summary(&edge_free_diff)
    );
    assert_diff_matches_expected(&before_units, &after_units, &diff);
});

type DiffSummary = (
    HashSet<String>,
    HashSet<String>,
    HashMap<String, Vec<ChangeKind>>,
    HashSet<String>,
);

fn normalize_units(namespace: &str, raw_units: &[ArbUnit]) -> Vec<SemanticUnit> {
    let mut used_names = HashSet::new();
    let mut units = Vec::new();

    for (index, raw) in raw_units.iter().enumerate() {
        let base_name = sanitize_name(&raw.name, &format!("{namespace}_unit_{index}"));
        let qualified_name = unique_name(base_name, &mut used_names);
        let file_path = sanitize_file(&raw.file, namespace, index);
        let body_hash = raw.body_hash;
        units.push(SemanticUnit {
            id: semantic_id(&qualified_name, &file_path, body_hash),
            kind: raw.kind.into(),
            name: qualified_name
                .rsplit("::")
                .next()
                .unwrap_or(qualified_name.as_str())
                .to_string(),
            qualified_name,
            file_path,
            byte_range: 0..0,
            signature_hash: raw.signature_hash,
            body_hash,
            dependencies: Vec::new(),
        });
    }

    units
}

fn build_graph(units: &[SemanticUnit], edges: &[ArbEdge]) -> CodeGraph {
    let mut graph = CodeGraph::new();
    for unit in units {
        graph.add_unit(unit.clone());
    }

    for edge in edges {
        if units.is_empty() {
            break;
        }
        let from = units[usize::from(edge.from) % units.len()].id;
        let to = units[usize::from(edge.to) % units.len()].id;
        graph.add_dep(from, to, edge.kind.into());
    }

    graph
}

fn classification_summary(diff: &nex_core::SemanticDiff) -> DiffSummary {
    let added = diff
        .added
        .iter()
        .map(|unit| unit.qualified_name.clone())
        .collect();
    let removed = diff
        .removed
        .iter()
        .map(|unit| unit.qualified_name.clone())
        .collect();
    let modified = diff
        .modified
        .iter()
        .map(|unit| (unit.after.qualified_name.clone(), unit.changes.clone()))
        .collect();
    let moved = diff
        .moved
        .iter()
        .map(|unit| unit.unit.qualified_name.clone())
        .collect();
    (added, removed, modified, moved)
}

fn assert_diff_matches_expected(
    before_units: &[SemanticUnit],
    after_units: &[SemanticUnit],
    diff: &nex_core::SemanticDiff,
) {
    let before_by_name: HashMap<_, _> = before_units
        .iter()
        .map(|unit| (unit.qualified_name.as_str(), unit))
        .collect();
    let after_by_name: HashMap<_, _> = after_units
        .iter()
        .map(|unit| (unit.qualified_name.as_str(), unit))
        .collect();

    let added_names: HashSet<_> = diff
        .added
        .iter()
        .map(|unit| unit.qualified_name.as_str())
        .collect();
    let removed_names: HashSet<_> = diff
        .removed
        .iter()
        .map(|unit| unit.qualified_name.as_str())
        .collect();
    let modified_names: HashSet<_> = diff
        .modified
        .iter()
        .map(|unit| unit.after.qualified_name.as_str())
        .collect();
    let moved_names: HashSet<_> = diff
        .moved
        .iter()
        .map(|unit| unit.unit.qualified_name.as_str())
        .collect();

    assert!(added_names.is_disjoint(&removed_names));
    assert!(added_names.is_disjoint(&modified_names));
    assert!(added_names.is_disjoint(&moved_names));
    assert!(removed_names.is_disjoint(&modified_names));
    assert!(removed_names.is_disjoint(&moved_names));
    assert!(modified_names.is_disjoint(&moved_names));

    for unit in &diff.added {
        assert!(!before_by_name.contains_key(unit.qualified_name.as_str()));
        assert!(after_by_name.contains_key(unit.qualified_name.as_str()));
    }

    for unit in &diff.removed {
        assert!(before_by_name.contains_key(unit.qualified_name.as_str()));
        assert!(!after_by_name.contains_key(unit.qualified_name.as_str()));
    }

    for modified in &diff.modified {
        let before = before_by_name
            .get(modified.after.qualified_name.as_str())
            .expect("modified unit must exist before");
        let after = after_by_name
            .get(modified.after.qualified_name.as_str())
            .expect("modified unit must exist after");

        assert_eq!(before.qualified_name, after.qualified_name);
        assert!(
            before.signature_hash != after.signature_hash || before.body_hash != after.body_hash
        );

        let mut expected_changes = Vec::new();
        if before.signature_hash != after.signature_hash {
            expected_changes.push(ChangeKind::SignatureChanged);
        }
        if before.body_hash != after.body_hash {
            expected_changes.push(ChangeKind::BodyChanged);
        }
        assert_eq!(modified.changes, expected_changes);
    }

    for moved in &diff.moved {
        let before = before_by_name
            .get(moved.unit.qualified_name.as_str())
            .expect("moved unit must exist before");
        let after = after_by_name
            .get(moved.unit.qualified_name.as_str())
            .expect("moved unit must exist after");

        assert_eq!(before.signature_hash, after.signature_hash);
        assert_eq!(before.body_hash, after.body_hash);
        assert_ne!(before.file_path, after.file_path);
        assert_eq!(moved.old_path, before.file_path);
        assert_eq!(moved.new_path, after.file_path);
    }

    for (name, before) in &before_by_name {
        match after_by_name.get(name) {
            None => assert!(removed_names.contains(name)),
            Some(after)
                if before.signature_hash == after.signature_hash
                    && before.body_hash == after.body_hash =>
            {
                if before.file_path != after.file_path {
                    assert!(moved_names.contains(name));
                } else {
                    assert!(!added_names.contains(name));
                    assert!(!removed_names.contains(name));
                    assert!(!modified_names.contains(name));
                    assert!(!moved_names.contains(name));
                }
            }
            Some(_) => assert!(modified_names.contains(name)),
        }
    }
}

fn sanitize_name(raw: &str, fallback: &str) -> String {
    let mut cleaned = String::new();
    for character in raw.chars() {
        if character.is_ascii_alphanumeric() || matches!(character, '_' | ':') {
            cleaned.push(character);
        } else if !cleaned.ends_with('_') {
            cleaned.push('_');
        }
    }
    let cleaned = cleaned.trim_matches('_');
    if cleaned.is_empty() {
        fallback.to_string()
    } else {
        cleaned.to_string()
    }
}

fn unique_name(base: String, used_names: &mut HashSet<String>) -> String {
    if used_names.insert(base.clone()) {
        return base;
    }

    let mut suffix = 1usize;
    loop {
        let candidate = format!("{base}_{suffix}");
        if used_names.insert(candidate.clone()) {
            return candidate;
        }
        suffix += 1;
    }
}

fn sanitize_file(raw: &str, namespace: &str, index: usize) -> PathBuf {
    let stem = sanitize_name(raw, &format!("{namespace}_file_{index}"));
    PathBuf::from(format!("{stem}.ts"))
}

fn semantic_id(qualified_name: &str, path: &Path, body_hash: u64) -> SemanticId {
    let digest =
        blake3::hash(format!("{}:{}:{}", qualified_name, path.display(), body_hash).as_bytes());
    *digest.as_bytes()
}

impl From<ArbUnitKind> for UnitKind {
    fn from(value: ArbUnitKind) -> Self {
        match value {
            ArbUnitKind::Function => UnitKind::Function,
            ArbUnitKind::Method => UnitKind::Method,
            ArbUnitKind::Class => UnitKind::Class,
            ArbUnitKind::Struct => UnitKind::Struct,
            ArbUnitKind::Interface => UnitKind::Interface,
            ArbUnitKind::Trait => UnitKind::Trait,
            ArbUnitKind::Enum => UnitKind::Enum,
            ArbUnitKind::Module => UnitKind::Module,
            ArbUnitKind::Constant => UnitKind::Constant,
        }
    }
}

impl From<ArbDepKind> for DepKind {
    fn from(value: ArbDepKind) -> Self {
        match value {
            ArbDepKind::Calls => DepKind::Calls,
            ArbDepKind::Imports => DepKind::Imports,
            ArbDepKind::Inherits => DepKind::Inherits,
            ArbDepKind::Implements => DepKind::Implements,
            ArbDepKind::Uses => DepKind::Uses,
        }
    }
}

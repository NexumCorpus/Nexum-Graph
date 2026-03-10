use nex_core::{DepKind, SemanticUnit, UnitKind};
use nex_graph::CodeGraph;
use proptest::prelude::*;
use std::collections::BTreeSet;
use std::path::PathBuf;

#[derive(Debug, Clone)]
struct UnitSpec {
    path_key: u8,
    signature_hash: u64,
    body_hash: u64,
}

fn unit_from_spec(index: usize, spec: &UnitSpec) -> SemanticUnit {
    let qualified_name = format!("unit::{index}");
    let id_input = format!("{qualified_name}:{}", file_path(spec.path_key));
    let id = *blake3::hash(id_input.as_bytes()).as_bytes();
    SemanticUnit {
        id,
        kind: UnitKind::Function,
        name: format!("unit_{index}"),
        qualified_name,
        file_path: PathBuf::from(file_path(spec.path_key)),
        byte_range: 0..64,
        signature_hash: spec.signature_hash,
        body_hash: spec.body_hash,
        dependencies: Vec::new(),
    }
}

fn file_path(path_key: u8) -> String {
    format!("src/file_{}.rs", path_key % 5)
}

fn build_graph(specs: &[UnitSpec], edges: &[(usize, usize, DepKind)]) -> CodeGraph {
    let mut graph = CodeGraph::new();
    let units: Vec<_> = specs
        .iter()
        .enumerate()
        .map(|(index, spec)| unit_from_spec(index, spec))
        .collect();

    for unit in &units {
        graph.add_unit(unit.clone());
    }
    for (from, to, kind) in edges {
        if *from < units.len() && *to < units.len() && from != to {
            graph.add_dep(units[*from].id, units[*to].id, *kind);
        }
    }

    graph
}

fn edge_strategy(max_nodes: usize) -> impl Strategy<Value = Vec<(usize, usize, DepKind)>> {
    prop::collection::vec(
        (
            0..max_nodes,
            0..max_nodes,
            prop_oneof![
                Just(DepKind::Calls),
                Just(DepKind::Imports),
                Just(DepKind::Inherits),
                Just(DepKind::Implements),
                Just(DepKind::Uses),
            ],
        ),
        0..24,
    )
}

fn spec_strategy() -> impl Strategy<Value = UnitSpec> {
    (any::<u8>(), any::<u64>(), any::<u64>()).prop_map(|(path_key, signature_hash, body_hash)| {
        UnitSpec {
            path_key,
            signature_hash,
            body_hash,
        }
    })
}

proptest! {
    #[test]
    fn diff_is_empty_for_identical_graphs_even_when_edges_vary(
        specs in prop::collection::vec(spec_strategy(), 0..10),
        edges in edge_strategy(10),
    ) {
        let before = build_graph(&specs, &edges);
        let after = build_graph(&specs, &edges);

        let diff = before.diff(&after);

        prop_assert!(diff.added.is_empty());
        prop_assert!(diff.removed.is_empty());
        prop_assert!(diff.modified.is_empty());
        prop_assert!(diff.moved.is_empty());
    }

    #[test]
    fn diff_buckets_are_disjoint_and_match_name_level_expectations(
        before_specs in prop::collection::vec(spec_strategy(), 0..10),
        after_specs in prop::collection::vec(spec_strategy(), 0..10),
    ) {
        let before = build_graph(&before_specs, &[]);
        let after = build_graph(&after_specs, &[]);
        let diff = before.diff(&after);

        let added: BTreeSet<_> = diff.added.iter().map(|unit| unit.qualified_name.clone()).collect();
        let removed: BTreeSet<_> = diff.removed.iter().map(|unit| unit.qualified_name.clone()).collect();
        let modified: BTreeSet<_> = diff.modified.iter().map(|unit| unit.after.qualified_name.clone()).collect();
        let moved: BTreeSet<_> = diff.moved.iter().map(|unit| unit.unit.qualified_name.clone()).collect();

        for name in &added {
            prop_assert!(!removed.contains(name));
            prop_assert!(!modified.contains(name));
            prop_assert!(!moved.contains(name));
        }
        for name in &removed {
            prop_assert!(!modified.contains(name));
            prop_assert!(!moved.contains(name));
        }
        for name in &modified {
            prop_assert!(!moved.contains(name));
        }

        let shared_len = before_specs.len().min(after_specs.len());
        for index in 0..shared_len {
            let name = format!("unit::{index}");
            let before_spec = &before_specs[index];
            let after_spec = &after_specs[index];
            let same_signature = before_spec.signature_hash == after_spec.signature_hash;
            let same_body = before_spec.body_hash == after_spec.body_hash;
            let same_path = before_spec.path_key % 5 == after_spec.path_key % 5;

            if same_signature && same_body && same_path {
                prop_assert!(!added.contains(&name));
                prop_assert!(!removed.contains(&name));
                prop_assert!(!modified.contains(&name));
                prop_assert!(!moved.contains(&name));
            } else if same_signature && same_body && !same_path {
                prop_assert!(moved.contains(&name));
                prop_assert!(!modified.contains(&name));
            } else {
                prop_assert!(modified.contains(&name));
                prop_assert!(!moved.contains(&name));
            }
        }

        for index in shared_len..before_specs.len() {
            let expected_name = format!("unit::{}", index);
            prop_assert!(removed.contains(&expected_name));
        }

        for index in shared_len..after_specs.len() {
            let expected_name = format!("unit::{}", index);
            prop_assert!(added.contains(&expected_name));
        }
    }
}

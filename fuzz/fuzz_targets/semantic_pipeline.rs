#![no_main]

use libfuzzer_sys::fuzz_target;
use nex_core::{DepKind, SemanticId, SemanticUnit};
use nex_graph::CodeGraph;
use nex_parse::SemanticExtractor;
use std::collections::HashSet;
use std::path::Path;

fuzz_target!(|data: &[u8]| {
    run_semantic_pipeline("fuzz.ts", nex_parse::typescript_extractor(), data);
    run_semantic_pipeline("fuzz.py", nex_parse::python_extractor(), data);
    run_semantic_pipeline("fuzz.rs", nex_parse::rust_extractor(), data);
});

fn run_semantic_pipeline(path_str: &str, extractor: Box<dyn SemanticExtractor>, source: &[u8]) {
    let path = Path::new(path_str);
    let units = extractor
        .extract(path, source)
        .expect("extractor should not fail on arbitrary bytes");
    let deps = extractor
        .dependencies(&units, source)
        .expect("dependency extraction should not fail on arbitrary bytes");

    let unit_ids: HashSet<_> = units.iter().map(|unit| unit.id).collect();
    for unit in &units {
        assert_eq!(unit.file_path, path);
        assert!(unit.byte_range.start <= unit.byte_range.end);
        assert!(unit.byte_range.end <= source.len());
        assert_eq!(unit.id, expected_id(unit));
    }

    for (from, to, _) in &deps {
        assert!(unit_ids.contains(from));
        assert!(unit_ids.contains(to));
    }

    let graph = build_graph(&units, &deps);
    let diff = graph.diff(&graph);
    assert!(diff.added.is_empty());
    assert!(diff.removed.is_empty());
    assert!(diff.modified.is_empty());
    assert!(diff.moved.is_empty());

    for unit in &units {
        assert!(graph.get(&unit.id).is_some());
        for dep in graph.deps_of(&unit.id) {
            assert!(unit_ids.contains(&dep.id));
        }
        for caller in graph.callers_of(&unit.id) {
            assert!(unit_ids.contains(&caller.id));
        }
    }
}

fn build_graph(units: &[SemanticUnit], deps: &[(SemanticId, SemanticId, DepKind)]) -> CodeGraph {
    let mut graph = CodeGraph::new();
    for unit in units {
        graph.add_unit(unit.clone());
    }
    for (from, to, kind) in deps {
        graph.add_dep(*from, *to, *kind);
    }
    graph
}

fn expected_id(unit: &SemanticUnit) -> SemanticId {
    let digest = blake3::hash(
        format!(
            "{}:{}:{}",
            unit.qualified_name,
            unit.file_path.display(),
            unit.body_hash
        )
        .as_bytes(),
    );
    *digest.as_bytes()
}

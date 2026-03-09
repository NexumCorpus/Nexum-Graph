//! Semantic diff pipeline: git -> parse -> graph -> diff.
//!
//! This module implements the 5-step pipeline from the spec:
//! 1. Git read (git2) -> collect source files by supported extension
//! 2. Parse (codex-parse) -> run SemanticExtractor per language
//! 3. Build CodeGraphs (codex-graph) -> one per ref
//! 4. Diff (CodeGraph::diff) -> produce SemanticDiff
//! 5. Output -> handled by the `output` module

use codex_core::{CodexError, CodexResult, SemanticDiff};
use codex_graph::CodeGraph;
use codex_parse::SemanticExtractor;
use git2::{ObjectType, TreeWalkMode, TreeWalkResult};
use std::path::Path;

/// Collect source files from a git tree at the given ref.
///
/// Returns `(relative_path, file_content_bytes)` pairs for files
/// whose extensions match one of the supported extractors.
pub fn collect_files_at_ref(
    repo: &git2::Repository,
    refspec: &str,
    extractors: &[Box<dyn SemanticExtractor>],
) -> CodexResult<Vec<(String, Vec<u8>)>> {
    let commit = repo
        .revparse_single(refspec)
        .and_then(|object| object.peel_to_commit())
        .map_err(|err| CodexError::Git(err.to_string()))?;
    let tree = commit
        .tree()
        .map_err(|err| CodexError::Git(err.to_string()))?;

    let mut files = Vec::new();
    let mut walk_error: Option<CodexError> = None;

    tree.walk(TreeWalkMode::PreOrder, |root, entry| {
        if entry.kind() != Some(ObjectType::Blob) {
            return TreeWalkResult::Ok;
        }

        let Some(name) = entry.name() else {
            return TreeWalkResult::Ok;
        };
        let full_path = format!("{root}{name}");
        let ext = Path::new(&full_path)
            .extension()
            .and_then(|ext| ext.to_str());
        let is_supported = ext.is_some_and(|ext| {
            extractors
                .iter()
                .any(|extractor| extractor.extensions().contains(&ext))
        });

        if !is_supported {
            return TreeWalkResult::Ok;
        }

        match repo.find_blob(entry.id()) {
            Ok(blob) => {
                files.push((full_path, blob.content().to_vec()));
                TreeWalkResult::Ok
            }
            Err(err) => {
                walk_error = Some(CodexError::Git(err.to_string()));
                TreeWalkResult::Abort
            }
        }
    })
    .map_err(|err| {
        walk_error
            .take()
            .unwrap_or_else(|| CodexError::Git(err.to_string()))
    })?;

    if let Some(err) = walk_error {
        return Err(err);
    }

    Ok(files)
}

/// Parse source files into a CodeGraph using the appropriate extractor.
///
/// For each file, selects the extractor whose `extensions()` match the
/// file's extension, calls `extract()` and `dependencies()`, then adds
/// units and edges to the graph.
pub fn build_graph(
    files: &[(String, Vec<u8>)],
    extractors: &[Box<dyn SemanticExtractor>],
) -> CodexResult<CodeGraph> {
    let mut graph = CodeGraph::new();

    for (path, content) in files {
        let Some(ext) = Path::new(path).extension().and_then(|ext| ext.to_str()) else {
            continue;
        };
        let Some(extractor) = extractors
            .iter()
            .find(|extractor| extractor.extensions().contains(&ext))
        else {
            continue;
        };

        let units = extractor.extract(Path::new(path), content)?;
        let dependencies = extractor.dependencies(&units, content)?;

        for unit in units {
            graph.add_unit(unit);
        }
        for (from_id, to_id, kind) in dependencies {
            graph.add_dep(from_id, to_id, kind);
        }
    }

    Ok(graph)
}

/// Run the full semantic diff pipeline between two git refs.
pub fn run_diff(repo_path: &Path, ref_a: &str, ref_b: &str) -> CodexResult<SemanticDiff> {
    let repo = git2::Repository::open(repo_path).map_err(|err| CodexError::Git(err.to_string()))?;
    let extractors: Vec<Box<dyn SemanticExtractor>> = vec![codex_parse::typescript_extractor()];

    let files_a = collect_files_at_ref(&repo, ref_a, &extractors)?;
    let files_b = collect_files_at_ref(&repo, ref_b, &extractors)?;
    let graph_a = build_graph(&files_a, &extractors)?;
    let graph_b = build_graph(&files_b, &extractors)?;

    Ok(graph_a.diff(&graph_b))
}

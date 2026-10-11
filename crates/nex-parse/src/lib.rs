//! nex-parse: Tree-sitter parsing, rowan CST bridge, semantic unit extraction.
//!
//! This crate owns:
//! - `SemanticExtractor` trait (language-agnostic extraction interface)
//! - Tree-sitter → Rowan bridge (first-of-its-kind in the Rust ecosystem)
//! - Per-language extractors (TypeScript P0, Python P0, Rust P1)
//! - `KindMap` generation from tree-sitter node-types.json
//!
//! Reviewing this file:
//! - This is the narrow shared boundary for all language extractors.
//! - `semantic_unit_id()` is the canonical semantic-id formula used across
//!   TypeScript, Python, and Rust extractors.
//! - If the extractor registry or id formula changes, update
//!   `CORE_INVARIANTS.md` and the extractor contract tests.

use nex_core::{CodexResult, DepKind, SemanticId, SemanticUnit};
use std::path::Path;

pub mod bridge;
pub mod python;
pub mod rust;
pub mod typescript;

// ─────────────────────────────────────────────────────────────────────────────
// SemanticExtractor Trait (SPEC CONTRACT — do not alter signature)
// ─────────────────────────────────────────────────────────────────────────────

/// Language-agnostic extraction interface.
/// Each language implements this with tree-sitter queries.
pub trait SemanticExtractor: Send + Sync {
    /// Supported file extensions (e.g., &["ts", "tsx"] for TypeScript).
    fn extensions(&self) -> &[&str];

    /// Parse source bytes, return extracted semantic units.
    fn extract(&self, path: &Path, source: &[u8]) -> CodexResult<Vec<SemanticUnit>>;

    /// Extract dependency edges between units.
    /// Returns (from_id, to_id, dependency_kind) triples.
    fn dependencies(
        &self,
        units: &[SemanticUnit],
        source: &[u8],
    ) -> CodexResult<Vec<(SemanticId, SemanticId, DepKind)>>;
}

// ─────────────────────────────────────────────────────────────────────────────
// Public API — extractor constructors
// ─────────────────────────────────────────────────────────────────────────────

/// Create a TypeScript semantic extractor.
pub fn typescript_extractor() -> Box<dyn SemanticExtractor> {
    Box::new(typescript::TypeScriptExtractor::new())
}

/// Create a Python semantic extractor.
pub fn python_extractor() -> Box<dyn SemanticExtractor> {
    Box::new(python::PythonExtractor::new())
}

/// Create a Rust semantic extractor.
pub fn rust_extractor() -> Box<dyn SemanticExtractor> {
    Box::new(rust::RustExtractor::new())
}

/// Create the default multi-language extractor set from the spec.
pub fn default_extractors() -> Vec<Box<dyn SemanticExtractor>> {
    vec![typescript_extractor(), python_extractor(), rust_extractor()]
}

/// Supported source file extensions across the default extractor set.
pub fn supported_extensions() -> &'static [&'static str] {
    &["ts", "tsx", "py", "rs"]
}

/// Return whether the extension is supported by any built-in extractor.
pub fn supports_extension(ext: &str) -> bool {
    supported_extensions().contains(&ext)
}

/// Create the built-in extractor that handles the given extension.
pub fn extractor_for_extension(ext: &str) -> Option<Box<dyn SemanticExtractor>> {
    match ext {
        "ts" | "tsx" => Some(typescript_extractor()),
        "py" => Some(python_extractor()),
        "rs" => Some(rust_extractor()),
        _ => None,
    }
}

/// Create the built-in extractor that handles the given path.
pub fn extractor_for_path(path: &Path) -> Option<Box<dyn SemanticExtractor>> {
    let ext = path.extension()?.to_str()?;
    extractor_for_extension(ext)
}

pub(crate) fn semantic_unit_id(qualified_name: &str, path: &Path, body_hash: u64) -> SemanticId {
    let digest =
        blake3::hash(format!("{}:{}:{}", qualified_name, path.display(), body_hash).as_bytes());
    *digest.as_bytes()
}

/// Work out which file path(s) a `dependencies()` source buffer belongs to.
///
/// `dependencies()` receives every unit in the snapshot (for cross-file
/// resolution) plus one file's source, but not that file's path. `analyzed`
/// are the units re-extracted from `source` under a placeholder path, so their
/// ids are not the real ones. A path qualifies when every analyzed unit, with
/// its id recomputed for that path, is present in `units`. Several paths
/// qualify only when files have byte-identical unit content.
///
/// Callers used to take `units.first().file_path` as the source's path, which
/// is only right for the first file of the snapshot: every other file then
/// produced no dependency edges at all.
pub(crate) fn resolve_source_paths<'a>(
    units: &[SemanticUnit],
    analyzed: impl IntoIterator<Item = &'a SemanticUnit>,
) -> Vec<std::path::PathBuf> {
    let analyzed: Vec<&SemanticUnit> = analyzed.into_iter().collect();
    let Some(probe) = analyzed.first() else {
        return Vec::new();
    };

    let mut candidates: Vec<&Path> = Vec::new();
    for unit in units {
        if unit.byte_range == probe.byte_range
            && unit.body_hash == probe.body_hash
            && unit.signature_hash == probe.signature_hash
            && unit.qualified_name == probe.qualified_name
            && !candidates.contains(&unit.file_path.as_path())
        {
            candidates.push(unit.file_path.as_path());
        }
    }
    if candidates.is_empty() {
        return Vec::new();
    }

    let known_ids: std::collections::HashSet<SemanticId> =
        units.iter().map(|unit| unit.id).collect();
    candidates
        .into_iter()
        .filter(|path| {
            analyzed.iter().all(|unit| {
                known_ids.contains(&semantic_unit_id(
                    &unit.qualified_name,
                    path,
                    unit.body_hash,
                ))
            })
        })
        .map(Path::to_path_buf)
        .collect()
}

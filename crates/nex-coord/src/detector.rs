//! Semantic conflict detection engine.
//!
//! Implements the Phase 1 pipeline from the spec:
//! 1. Find merge base via git2
//! 2. Build three CodeGraphs: base, branch_a, branch_b
//! 3. Compute SemanticDiff base->A and base->B
//! 4. Cross-reference: for each removed/renamed/signature-changed unit
//!    in diff_a, check if graph_b depends on the old version. Vice versa.
//! 5. For units modified in both diffs, classify as independent, additive,
//!    or conflicting.

use git2::{ObjectType, TreeWalkMode, TreeWalkResult};
use nex_core::{
    ChangeKind, CodexError, CodexResult, ConflictKind, ConflictReport, SemanticConflict,
    SemanticDiff, SemanticUnit, Severity,
};
use nex_graph::CodeGraph;
use nex_parse::SemanticExtractor;
use std::collections::HashMap;
use std::path::Path;

/// Semantic conflict detector.
///
/// Stateless engine - call `detect()` with a repo and two branch refs.
pub struct ConflictDetector;

impl ConflictDetector {
    /// Detect semantic conflicts between two branches.
    ///
    /// Internally computes the merge base, builds three CodeGraphs
    /// (base, branch_a, branch_b), diffs them, and cross-references
    /// the changes to find conflicts.
    pub fn detect(repo_path: &Path, branch_a: &str, branch_b: &str) -> CodexResult<ConflictReport> {
        let repo =
            git2::Repository::open(repo_path).map_err(|err| CodexError::Git(err.to_string()))?;
        let merge_base = find_merge_base(&repo, branch_a, branch_b)?;
        let extractors: Vec<Box<dyn SemanticExtractor>> = nex_parse::default_extractors();

        let graph_base = build_graph_at_ref(&repo, &merge_base.to_string(), &extractors)?;
        let graph_a = build_graph_at_ref(&repo, branch_a, &extractors)?;
        let graph_b = build_graph_at_ref(&repo, branch_b, &extractors)?;

        let diff_a = graph_base.diff(&graph_a);
        let diff_b = graph_base.diff(&graph_b);
        let mut conflicts = cross_reference(&diff_a, &diff_b, &graph_base, &graph_a, &graph_b);
        conflicts.sort_by(|left, right| {
            severity_rank(left.severity)
                .cmp(&severity_rank(right.severity))
                .then(left.description.cmp(&right.description))
        });

        Ok(ConflictReport {
            conflicts,
            branch_a: branch_a.to_string(),
            branch_b: branch_b.to_string(),
            merge_base: merge_base.to_string(),
        })
    }
}

/// Find the merge base between two refs.
///
/// Returns the merge base OID as a hex string.
pub(crate) fn find_merge_base(
    repo: &git2::Repository,
    ref_a: &str,
    ref_b: &str,
) -> CodexResult<git2::Oid> {
    let oid_a = repo
        .revparse_single(ref_a)
        .and_then(|object| object.peel_to_commit())
        .map(|commit| commit.id())
        .map_err(|err| CodexError::Git(err.to_string()))?;
    let oid_b = repo
        .revparse_single(ref_b)
        .and_then(|object| object.peel_to_commit())
        .map(|commit| commit.id())
        .map_err(|err| CodexError::Git(err.to_string()))?;

    repo.merge_base(oid_a, oid_b)
        .map_err(|err| CodexError::Git(err.to_string()))
}

/// Build a CodeGraph from all supported files at the given ref.
///
/// Reuses the same collect_files + parse + graph pipeline from nex-cli.
pub(crate) fn build_graph_at_ref(
    repo: &git2::Repository,
    refspec: &str,
    extractors: &[Box<dyn SemanticExtractor>],
) -> CodexResult<CodeGraph> {
    let files = collect_files_at_ref(repo, refspec, extractors)?;
    build_graph_from_files(&files, extractors)
}

/// Cross-reference two diffs to detect semantic conflicts.
///
/// This is the core conflict detection algorithm:
/// - For each removed unit in diff_a, check if graph_b has callers -> DeletedDependency
/// - For each removed unit in diff_b, check if graph_a has callers -> DeletedDependency
/// - For each signature-changed unit in diff_a, check if graph_b calls it -> SignatureMismatch
/// - For each signature-changed unit in diff_b, check if graph_a calls it -> SignatureMismatch
///
///   A caller only counts as "branch X depends on it" when X's copy of the
///   caller can survive the merge: a caller X left identical to base while the
///   other branch modified or removed it is superseded by the other branch's
///   version in a 3-way merge, so it is skipped (see `superseded_in_merge`).
/// - For units modified in both diffs (by qualified_name):
///   - Both modify body -> ConcurrentBodyEdit (Warning)
///   - Both modify signature -> ConcurrentBodyEdit (Error)
/// - For units added in both diffs with same qualified_name -> NamingCollision
pub(crate) fn cross_reference(
    diff_a: &SemanticDiff,
    diff_b: &SemanticDiff,
    graph_base: &CodeGraph,
    graph_a: &CodeGraph,
    graph_b: &CodeGraph,
) -> Vec<SemanticConflict> {
    let mut conflicts = Vec::new();
    let a_to_b = BranchPair {
        base: graph_base,
        changing: graph_a,
        dependent: graph_b,
        changing_label: "A",
        dependent_label: "B",
    };
    let b_to_a = BranchPair {
        base: graph_base,
        changing: graph_b,
        dependent: graph_a,
        changing_label: "B",
        dependent_label: "A",
    };

    push_deleted_dependency_conflicts(&mut conflicts, &diff_a.removed, &a_to_b);
    push_deleted_dependency_conflicts(&mut conflicts, &diff_b.removed, &b_to_a);
    push_signature_mismatch_conflicts(&mut conflicts, &diff_a.modified, &a_to_b);
    push_signature_mismatch_conflicts(&mut conflicts, &diff_b.modified, &b_to_a);
    push_concurrent_modification_conflicts(&mut conflicts, diff_a, diff_b);
    push_naming_collision_conflicts(&mut conflicts, diff_a, diff_b);

    conflicts
}

fn collect_files_at_ref(
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

fn build_graph_from_files(
    files: &[(String, Vec<u8>)],
    extractors: &[Box<dyn SemanticExtractor>],
) -> CodexResult<CodeGraph> {
    let mut parsed_files = Vec::new();
    let mut all_units = Vec::new();

    for (path, content) in files {
        let Some(ext) = Path::new(path).extension().and_then(|ext| ext.to_str()) else {
            continue;
        };
        let Some((extractor_index, extractor)) = extractors
            .iter()
            .enumerate()
            .find(|(_, extractor)| extractor.extensions().contains(&ext))
        else {
            continue;
        };

        let units = extractor.extract(Path::new(path), content)?;
        all_units.extend(units.iter().cloned());
        parsed_files.push((extractor_index, path.clone(), content.clone(), units));
    }

    let mut graph = CodeGraph::new();
    let mut all_dependencies = Vec::new();

    for (extractor_index, _path, content, _units) in &parsed_files {
        let dependencies = extractors[*extractor_index].dependencies(&all_units, content)?;
        all_dependencies.extend(dependencies);
    }

    for unit in all_units {
        graph.add_unit(unit);
    }
    for (from_id, to_id, kind) in all_dependencies {
        graph.add_dep(from_id, to_id, kind);
    }

    Ok(graph)
}

/// The two branch snapshots involved in one direction of cross-referencing:
/// `changing` removed or re-signed a unit, `dependent` may still call it.
struct BranchPair<'a> {
    base: &'a CodeGraph,
    changing: &'a CodeGraph,
    dependent: &'a CodeGraph,
    changing_label: &'a str,
    dependent_label: &'a str,
}

/// Whether the dependent branch's copy of `caller` is superseded in a 3-way
/// merge: the dependent branch left it identical to base (same file, same
/// signature and body) while the changing branch modified or removed it. Git
/// then keeps the changing branch's version, so the dependent branch's stale
/// copy is not what calls the changed unit after the merge.
///
/// A caller the dependent branch added or modified is never superseded, and
/// neither is one that both branches left untouched.
fn superseded_in_merge(caller: &SemanticUnit, pair: &BranchPair<'_>) -> bool {
    let Some(base_caller) = find_unit_at(pair.base, caller) else {
        return false;
    };
    if !same_content(base_caller, caller) {
        return false;
    }
    match find_unit_at(pair.changing, caller) {
        Some(changing_caller) => !same_content(base_caller, changing_caller),
        None => true,
    }
}

fn find_unit_at<'a>(graph: &'a CodeGraph, like: &SemanticUnit) -> Option<&'a SemanticUnit> {
    graph
        .units()
        .into_iter()
        .find(|unit| unit.qualified_name == like.qualified_name && unit.file_path == like.file_path)
}

fn same_content(left: &SemanticUnit, right: &SemanticUnit) -> bool {
    left.signature_hash == right.signature_hash && left.body_hash == right.body_hash
}

fn push_deleted_dependency_conflicts(
    conflicts: &mut Vec<SemanticConflict>,
    removed_units: &[SemanticUnit],
    pair: &BranchPair<'_>,
) {
    let other_graph = pair.dependent;
    let deleted_branch = pair.changing_label;
    let dependent_branch = pair.dependent_label;
    for removed_unit in removed_units {
        let Some(other_unit) = find_unit_by_name(other_graph, &removed_unit.qualified_name) else {
            continue;
        };

        for caller in other_graph.callers_of(&other_unit.id) {
            if superseded_in_merge(caller, pair) {
                continue;
            }
            conflicts.push(SemanticConflict {
                kind: ConflictKind::DeletedDependency {
                    deleted: removed_unit.id,
                    dependent: caller.id,
                },
                severity: Severity::Error,
                unit_a: removed_unit.clone(),
                unit_b: caller.clone(),
                description: format!(
                    "Branch {deleted_branch} deleted `{}` but branch {dependent_branch} depends on it via `{}`",
                    removed_unit.name, caller.name
                ),
                suggestion: Some(format!(
                    "Consider keeping `{}` or updating callers in branch {dependent_branch}",
                    removed_unit.name
                )),
            });
        }
    }
}

fn push_signature_mismatch_conflicts(
    conflicts: &mut Vec<SemanticConflict>,
    modified_units: &[nex_core::ModifiedUnit],
    pair: &BranchPair<'_>,
) {
    let other_graph = pair.dependent;
    let changed_branch = pair.changing_label;
    let caller_branch = pair.dependent_label;
    for modified in modified_units {
        if !modified.changes.contains(&ChangeKind::SignatureChanged) {
            continue;
        }
        // Name-based diffing pairs same-named units from different files (bare
        // Python/TS names collide across modules). If the changing branch still
        // holds the base unit untouched at its original file, the "signature
        // change" is a new same-named unit elsewhere, not a change to this one.
        if find_unit_at(pair.changing, &modified.before)
            .is_some_and(|unit| same_content(unit, &modified.before))
        {
            continue;
        }

        let Some(other_unit) = find_unit_at(other_graph, &modified.before)
            .or_else(|| find_unit_by_name(other_graph, &modified.after.qualified_name))
        else {
            continue;
        };

        for caller in other_graph.callers_of(&other_unit.id) {
            if superseded_in_merge(caller, pair) {
                continue;
            }
            conflicts.push(SemanticConflict {
                kind: ConflictKind::SignatureMismatch {
                    function: modified.after.id,
                    caller: caller.id,
                },
                severity: Severity::Error,
                unit_a: modified.after.clone(),
                unit_b: caller.clone(),
                description: format!(
                    "Branch {changed_branch} changed signature of `{}` but branch {caller_branch} calls it",
                    modified.after.name
                ),
                suggestion: Some(format!(
                    "Update callers in branch {caller_branch} to match new signature"
                )),
            });
        }
    }
}

fn push_concurrent_modification_conflicts(
    conflicts: &mut Vec<SemanticConflict>,
    diff_a: &SemanticDiff,
    diff_b: &SemanticDiff,
) {
    let modified_a: HashMap<&str, &nex_core::ModifiedUnit> = diff_a
        .modified
        .iter()
        .map(|modified| (modified.after.qualified_name.as_str(), modified))
        .collect();
    let modified_b: HashMap<&str, &nex_core::ModifiedUnit> = diff_b
        .modified
        .iter()
        .map(|modified| (modified.after.qualified_name.as_str(), modified))
        .collect();

    for (name, modified_a_unit) in modified_a {
        let Some(modified_b_unit) = modified_b.get(name).copied() else {
            continue;
        };

        let a_signature = modified_a_unit
            .changes
            .contains(&ChangeKind::SignatureChanged);
        let b_signature = modified_b_unit
            .changes
            .contains(&ChangeKind::SignatureChanged);
        let a_body = modified_a_unit.changes.contains(&ChangeKind::BodyChanged);
        let b_body = modified_b_unit.changes.contains(&ChangeKind::BodyChanged);

        let severity = if a_signature && b_signature {
            Severity::Error
        } else if a_body && b_body {
            Severity::Warning
        } else {
            continue;
        };

        conflicts.push(SemanticConflict {
            kind: ConflictKind::ConcurrentBodyEdit {
                unit: modified_a_unit.after.id,
            },
            severity,
            unit_a: modified_a_unit.after.clone(),
            unit_b: modified_b_unit.after.clone(),
            description: format!("Both branches modified `{name}`"),
            suggestion: Some("Manual merge required".to_string()),
        });
    }
}

fn push_naming_collision_conflicts(
    conflicts: &mut Vec<SemanticConflict>,
    diff_a: &SemanticDiff,
    diff_b: &SemanticDiff,
) {
    let added_a: HashMap<&str, &SemanticUnit> = diff_a
        .added
        .iter()
        .map(|unit| (unit.qualified_name.as_str(), unit))
        .collect();
    let added_b: HashMap<&str, &SemanticUnit> = diff_b
        .added
        .iter()
        .map(|unit| (unit.qualified_name.as_str(), unit))
        .collect();

    for (name, unit_a) in added_a {
        let Some(unit_b) = added_b.get(name).copied() else {
            continue;
        };

        conflicts.push(SemanticConflict {
            kind: ConflictKind::NamingCollision {
                name: name.to_string(),
            },
            severity: Severity::Error,
            unit_a: unit_a.clone(),
            unit_b: unit_b.clone(),
            description: format!("Both branches added `{name}`"),
            suggestion: Some("Rename one of the functions to avoid collision".to_string()),
        });
    }
}

fn find_unit_by_name<'a>(graph: &'a CodeGraph, name: &str) -> Option<&'a SemanticUnit> {
    graph
        .units()
        .into_iter()
        .find(|unit| unit.qualified_name == name)
}

fn severity_rank(severity: Severity) -> u8 {
    match severity {
        Severity::Error => 0,
        Severity::Warning => 1,
        Severity::Info => 2,
    }
}

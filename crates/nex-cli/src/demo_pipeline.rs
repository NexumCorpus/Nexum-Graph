use crate::{auth_pipeline, coordination_pipeline, eventlog_pipeline, pipeline};
use git2::Repository;
use nex_core::{ChangeKind, CodexError, CodexResult, SemanticDiff};
use nex_parse::SemanticExtractor;
use serde::Serialize;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize)]
pub struct DemoReport {
    pub repo_path: PathBuf,
    pub head_ref: String,
    pub head_commit: String,
    pub base_ref: String,
    pub detected_languages: Vec<String>,
    pub indexed_files: usize,
    pub semantic_units: usize,
    pub dependency_edges: usize,
    pub current_diff: DemoDiffPreview,
    pub active_locks: usize,
    pub event_count: usize,
    pub auth_configured: bool,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DemoDiffPreview {
    pub available: bool,
    pub added: usize,
    pub removed: usize,
    pub modified: usize,
    pub moved: usize,
    pub highlights: Vec<String>,
    pub unavailable_reason: Option<String>,
}

pub async fn run_demo(repo_path: &Path, base_ref: &str, head_ref: &str) -> CodexResult<DemoReport> {
    let repo = Repository::open(repo_path).map_err(|err| CodexError::Git(err.to_string()))?;
    let extractors: Vec<Box<dyn SemanticExtractor>> = nex_parse::default_extractors();
    let files = pipeline::collect_files_at_ref(&repo, head_ref, &extractors)?;
    let graph = pipeline::build_graph(&files, &extractors)?;

    let mut warnings = Vec::new();
    let active_locks = match coordination_pipeline::run_locks(repo_path) {
        Ok(entries) => entries.len(),
        Err(err) => {
            warnings.push(format!("lock state unavailable: {err}"));
            0
        }
    };
    let event_count = match eventlog_pipeline::run_log(repo_path, None).await {
        Ok(events) => events.len(),
        Err(err) => {
            warnings.push(format!("event log unavailable: {err}"));
            0
        }
    };
    let auth_configured = match auth_pipeline::auth_status(repo_path, None) {
        Ok(status) => status.exists,
        Err(err) => {
            warnings.push(format!("auth status unavailable: {err}"));
            false
        }
    };

    Ok(DemoReport {
        repo_path: repo_path.to_path_buf(),
        head_ref: head_ref.to_string(),
        head_commit: resolve_commit_label(&repo, head_ref)?,
        base_ref: base_ref.to_string(),
        detected_languages: detect_languages(&files),
        indexed_files: files.len(),
        semantic_units: graph.unit_count(),
        dependency_edges: graph.edge_count(),
        current_diff: build_diff_preview(repo_path, base_ref, head_ref),
        active_locks,
        event_count,
        auth_configured,
        warnings,
    })
}

fn build_diff_preview(repo_path: &Path, base_ref: &str, head_ref: &str) -> DemoDiffPreview {
    match pipeline::run_diff(repo_path, base_ref, head_ref) {
        Ok(diff) => DemoDiffPreview {
            available: true,
            added: diff.added.len(),
            removed: diff.removed.len(),
            modified: diff.modified.len(),
            moved: diff.moved.len(),
            highlights: diff_highlights(&diff),
            unavailable_reason: None,
        },
        Err(err) => DemoDiffPreview {
            available: false,
            added: 0,
            removed: 0,
            modified: 0,
            moved: 0,
            highlights: Vec::new(),
            unavailable_reason: Some(err.to_string()),
        },
    }
}

fn resolve_commit_label(repo: &Repository, head_ref: &str) -> CodexResult<String> {
    let commit = repo
        .revparse_single(head_ref)
        .and_then(|object| object.peel_to_commit())
        .map_err(|err| CodexError::Git(err.to_string()))?;
    let short = commit.id().to_string().chars().take(7).collect::<String>();
    Ok(format!("{head_ref} ({short})"))
}

fn detect_languages(files: &[(String, Vec<u8>)]) -> Vec<String> {
    let mut languages = BTreeSet::new();
    for (path, _) in files {
        match Path::new(path).extension().and_then(|ext| ext.to_str()) {
            Some("ts") | Some("tsx") | Some("js") | Some("jsx") => {
                languages.insert("TypeScript/JavaScript".to_string());
            }
            Some("py") => {
                languages.insert("Python".to_string());
            }
            Some("rs") => {
                languages.insert("Rust".to_string());
            }
            _ => {}
        }
    }
    languages.into_iter().collect()
}

fn diff_highlights(diff: &SemanticDiff) -> Vec<String> {
    let mut highlights = Vec::new();

    for unit in diff.added.iter().take(2) {
        highlights.push(format!(
            "+ {:?} {} ({})",
            unit.kind,
            unit.qualified_name,
            unit.file_path.display()
        ));
    }
    for modified in diff.modified.iter().take(2) {
        highlights.push(format!(
            "~ {:?} {} [{}]",
            modified.after.kind,
            modified.after.qualified_name,
            join_changes(&modified.changes)
        ));
    }
    for unit in diff.removed.iter().take(1) {
        highlights.push(format!(
            "- {:?} {} ({})",
            unit.kind,
            unit.qualified_name,
            unit.file_path.display()
        ));
    }
    for moved in diff.moved.iter().take(1) {
        highlights.push(format!(
            "-> {:?} {} ({} -> {})",
            moved.unit.kind,
            moved.unit.qualified_name,
            moved.old_path.display(),
            moved.new_path.display()
        ));
    }

    highlights
}

fn join_changes(changes: &[ChangeKind]) -> String {
    changes
        .iter()
        .map(|change| match change {
            ChangeKind::SignatureChanged => "signature",
            ChangeKind::BodyChanged => "body",
            ChangeKind::DocChanged => "docs",
        })
        .collect::<Vec<_>>()
        .join(", ")
}

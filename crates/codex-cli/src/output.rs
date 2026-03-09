//! Output formatting for semantic diffs.
//!
//! Supports three formats:
//! - `json`: Machine-readable JSON via serde_json
//! - `text`: Human-readable summary
//! - `github`: GitHub-flavored markdown for PR comments

use codex_core::{ChangeKind, SemanticDiff};
use std::fmt::Write;

/// Format a SemanticDiff according to the requested format string.
///
/// Supported formats: "json", "text", "github".
/// Unknown formats fall back to "text".
pub fn format_diff(diff: &SemanticDiff, format: &str) -> String {
    match format {
        "json" => format_json(diff),
        "github" => format_github(diff),
        _ => format_text(diff),
    }
}

/// Serialize the diff as pretty-printed JSON.
fn format_json(diff: &SemanticDiff) -> String {
    serde_json::to_string_pretty(diff).unwrap_or_else(|err| format!("{{\"error\": \"{err}\"}}"))
}

/// Render a human-readable text summary.
fn format_text(diff: &SemanticDiff) -> String {
    let mut output = String::new();
    let _ = writeln!(output, "Semantic Diff Summary");
    let _ = writeln!(output, "=====================");
    let _ = writeln!(output, "Added:    {}", diff.added.len());
    let _ = writeln!(output, "Removed:  {}", diff.removed.len());
    let _ = writeln!(output, "Modified: {}", diff.modified.len());
    let _ = writeln!(output, "Moved:    {}", diff.moved.len());

    if !diff.added.is_empty() {
        let _ = writeln!(output);
        let _ = writeln!(output, "Added:");
        for unit in &diff.added {
            let _ = writeln!(
                output,
                "  + {:?} {} ({})",
                unit.kind,
                unit.qualified_name,
                unit.file_path.display()
            );
        }
    }

    if !diff.removed.is_empty() {
        let _ = writeln!(output);
        let _ = writeln!(output, "Removed:");
        for unit in &diff.removed {
            let _ = writeln!(
                output,
                "  - {:?} {} ({})",
                unit.kind,
                unit.qualified_name,
                unit.file_path.display()
            );
        }
    }

    if !diff.modified.is_empty() {
        let _ = writeln!(output);
        let _ = writeln!(output, "Modified:");
        for modified in &diff.modified {
            let changes = join_changes(&modified.changes);
            let _ = writeln!(
                output,
                "  ~ {:?} {} [{}]",
                modified.after.kind, modified.after.qualified_name, changes
            );
        }
    }

    if !diff.moved.is_empty() {
        let _ = writeln!(output);
        let _ = writeln!(output, "Moved:");
        for moved in &diff.moved {
            let _ = writeln!(
                output,
                "  -> {:?} {}: {} -> {}",
                moved.unit.kind,
                moved.unit.qualified_name,
                moved.old_path.display(),
                moved.new_path.display()
            );
        }
    }

    output
}

/// Render GitHub-flavored markdown for PR comments.
fn format_github(diff: &SemanticDiff) -> String {
    let mut output = String::new();
    let _ = writeln!(output, "# Semantic Diff");
    let _ = writeln!(output);
    let _ = writeln!(output, "| Category | Count |");
    let _ = writeln!(output, "|----------|-------|");
    let _ = writeln!(output, "| Added | {} |", diff.added.len());
    let _ = writeln!(output, "| Removed | {} |", diff.removed.len());
    let _ = writeln!(output, "| Modified | {} |", diff.modified.len());
    let _ = writeln!(output, "| Moved | {} |", diff.moved.len());

    if !diff.added.is_empty() {
        let _ = writeln!(output);
        let _ = writeln!(output, "## Added");
        let _ = writeln!(output, "| Kind | Name | File |");
        let _ = writeln!(output, "|------|------|------|");
        for unit in &diff.added {
            let _ = writeln!(
                output,
                "| {:?} | `{}` | `{}` |",
                unit.kind,
                unit.qualified_name,
                unit.file_path.display()
            );
        }
    }

    if !diff.removed.is_empty() {
        let _ = writeln!(output);
        let _ = writeln!(output, "## Removed");
        let _ = writeln!(output, "| Kind | Name | File |");
        let _ = writeln!(output, "|------|------|------|");
        for unit in &diff.removed {
            let _ = writeln!(
                output,
                "| {:?} | `{}` | `{}` |",
                unit.kind,
                unit.qualified_name,
                unit.file_path.display()
            );
        }
    }

    if !diff.modified.is_empty() {
        let _ = writeln!(output);
        let _ = writeln!(output, "## Modified");
        let _ = writeln!(output, "| Kind | Name | Changes |");
        let _ = writeln!(output, "|------|------|---------|");
        for modified in &diff.modified {
            let _ = writeln!(
                output,
                "| {:?} | `{}` | {} |",
                modified.after.kind,
                modified.after.qualified_name,
                join_changes(&modified.changes)
            );
        }
    }

    if !diff.moved.is_empty() {
        let _ = writeln!(output);
        let _ = writeln!(output, "## Moved");
        let _ = writeln!(output, "| Kind | Name | From | To |");
        let _ = writeln!(output, "|------|------|------|-----|");
        for moved in &diff.moved {
            let _ = writeln!(
                output,
                "| {:?} | `{}` | `{}` | `{}` |",
                moved.unit.kind,
                moved.unit.qualified_name,
                moved.old_path.display(),
                moved.new_path.display()
            );
        }
    }

    output
}

fn join_changes(changes: &[ChangeKind]) -> String {
    changes
        .iter()
        .map(change_label)
        .collect::<Vec<_>>()
        .join(", ")
}

fn change_label(change: &ChangeKind) -> &'static str {
    match change {
        ChangeKind::SignatureChanged => "signature",
        ChangeKind::BodyChanged => "body",
        ChangeKind::DocChanged => "docs",
    }
}

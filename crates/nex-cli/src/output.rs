//! Output formatting for semantic diffs.
//!
//! Supports multiple formats:
//! - `json`: Machine-readable JSON via serde_json
//! - `text`: Human-readable summary
//! - `github`: GitHub-flavored markdown for PR comments
//! - `html`: Visual report for sharing and artifact publishing

use crate::audit_pipeline::AuditVerificationReport;
use crate::auth_pipeline::{
    AuthConfigMode, AuthInitResult, AuthIssueResult, AuthRevokeResult, AuthStatus,
};
use crate::check_pipeline::{CheckHookInstallResult, CheckHookInstallStatus};
use crate::demo_pipeline::DemoReport;
use crate::start_pipeline::{StartReport, StartStepStatus};
use nex_core::{ChangeKind, ConflictKind, ConflictReport, SemanticDiff, Severity};
use std::fmt::Write;

pub fn format_demo_report(report: &DemoReport, format: &str) -> String {
    match format {
        "json" => serde_json::to_string_pretty(report)
            .unwrap_or_else(|err| format!("{{\"error\": \"{err}\"}}")),
        "html" => format_demo_report_html(report),
        _ => {
            let mut output = String::new();
            let _ = writeln!(output, "Nexum Graph Demo");
            let _ = writeln!(output, "================");
            let _ = writeln!(output, "Repo: {}", report.repo_path.display());
            let _ = writeln!(output, "Head: {}", report.head_commit);
            let _ = writeln!(
                output,
                "Languages detected: {}",
                if report.detected_languages.is_empty() {
                    "none".to_string()
                } else {
                    report.detected_languages.join(", ")
                }
            );
            let _ = writeln!(output, "Indexed files: {}", report.indexed_files);
            let _ = writeln!(output, "Semantic units: {}", report.semantic_units);
            let _ = writeln!(output, "Dependency edges: {}", report.dependency_edges);

            let _ = writeln!(output);
            let _ = writeln!(
                output,
                "Current semantic diff: {} -> {}",
                report.base_ref, report.head_ref
            );
            let _ = writeln!(output, "--------------------------------");
            if report.current_diff.available {
                let _ = writeln!(output, "Added: {}", report.current_diff.added);
                let _ = writeln!(output, "Removed: {}", report.current_diff.removed);
                let _ = writeln!(output, "Modified: {}", report.current_diff.modified);
                let _ = writeln!(output, "Moved: {}", report.current_diff.moved);
                if !report.current_diff.highlights.is_empty() {
                    let _ = writeln!(output);
                    let _ = writeln!(output, "Highlights:");
                    for highlight in &report.current_diff.highlights {
                        let _ = writeln!(output, "  {highlight}");
                    }
                }
            } else if let Some(reason) = &report.current_diff.unavailable_reason {
                let _ = writeln!(output, "Unavailable: {reason}");
            }

            let _ = writeln!(output);
            let _ = writeln!(output, "Workspace state");
            let _ = writeln!(output, "---------------");
            let _ = writeln!(output, "Active locks: {}", report.active_locks);
            let _ = writeln!(output, "Event log entries: {}", report.event_count);
            let _ = writeln!(
                output,
                "Auth configured: {}",
                if report.auth_configured { "yes" } else { "no" }
            );

            if !report.warnings.is_empty() {
                let _ = writeln!(output);
                let _ = writeln!(output, "Warnings:");
                for warning in &report.warnings {
                    let _ = writeln!(output, "  - {warning}");
                }
            }

            let _ = writeln!(output);
            let _ = writeln!(output, "Next:");
            let _ = writeln!(output, "  nex diff {} {}", report.base_ref, report.head_ref);
            let _ = writeln!(output, "  nex serve --host 127.0.0.1 --port 4000");
            output
        }
    }
}

pub fn format_start_report(report: &StartReport, format: &str) -> String {
    match format {
        "json" => serde_json::to_string_pretty(report)
            .unwrap_or_else(|err| format!("{{\"error\": \"{err}\"}}")),
        "html" => format_start_report_html(report),
        _ => {
            let mut output = String::new();
            let _ = writeln!(output, "Nexum Graph Start");
            let _ = writeln!(output, "=================");
            let _ = writeln!(output, "Repo: {}", report.repo_path.display());
            let _ = writeln!(
                output,
                "Languages: {}",
                if report.demo.detected_languages.is_empty() {
                    "none".to_string()
                } else {
                    report.demo.detected_languages.join(", ")
                }
            );
            let _ = writeln!(output, "Indexed files: {}", report.demo.indexed_files);
            let _ = writeln!(output, "Semantic units: {}", report.demo.semantic_units);
            let _ = writeln!(output, "Dependency edges: {}", report.demo.dependency_edges);
            let _ = writeln!(
                output,
                "Current diff preview: {}",
                if report.demo.current_diff.available {
                    format!(
                        "ready ({} added, {} modified, {} removed, {} moved)",
                        report.demo.current_diff.added,
                        report.demo.current_diff.modified,
                        report.demo.current_diff.removed,
                        report.demo.current_diff.moved
                    )
                } else {
                    "unavailable".to_string()
                }
            );
            let _ = writeln!(
                output,
                "Merge guard: {} ({})",
                if report.hook_installed {
                    if report.hook_healthy {
                        "installed"
                    } else {
                        "custom hook present"
                    }
                } else {
                    "not installed"
                },
                report.hook_path.display()
            );
            let _ = writeln!(
                output,
                "Server auth: {}",
                if report.auth_configured {
                    "configured"
                } else {
                    "not configured"
                }
            );

            let _ = writeln!(output);
            let _ = writeln!(output, "Next steps");
            let _ = writeln!(output, "----------");
            for (index, step) in report.next_steps.iter().enumerate() {
                let _ = writeln!(
                    output,
                    "{}. [{}] {}",
                    index + 1,
                    start_step_status_label(step.status),
                    step.title
                );
                let _ = writeln!(output, "   {}", step.reason);
                let _ = writeln!(output, "   {}", step.command);
            }

            if !report.demo.current_diff.highlights.is_empty() {
                let _ = writeln!(output);
                let _ = writeln!(output, "Snapshot highlights");
                let _ = writeln!(output, "-----------------");
                for highlight in &report.demo.current_diff.highlights {
                    let _ = writeln!(output, "  {highlight}");
                }
            }

            if !report.demo.warnings.is_empty() {
                let _ = writeln!(output);
                let _ = writeln!(output, "Warnings");
                let _ = writeln!(output, "--------");
                for warning in &report.demo.warnings {
                    let _ = writeln!(output, "  - {warning}");
                }
            }

            output
        }
    }
}

/// Format a SemanticDiff according to the requested format string.
///
/// Supported formats: "json", "text", and "github".
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

/// Format a ConflictReport according to the requested format string.
///
/// Supported formats: "json", "text", "github".
/// Unknown formats fall back to "text".
pub fn format_report(report: &ConflictReport, format: &str) -> String {
    match format {
        "json" => serde_json::to_string_pretty(report)
            .unwrap_or_else(|err| format!("{{\"error\": \"{err}\"}}")),
        "github" => format_report_github(report),
        "html" => format_report_html(report),
        _ => format_report_text(report),
    }
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

fn format_report_text(report: &ConflictReport) -> String {
    let mut output = String::new();
    let _ = writeln!(
        output,
        "Conflict Check: {} vs {}",
        report.branch_a, report.branch_b
    );
    let _ = writeln!(output, "Merge base: {}", report.merge_base);
    let _ = writeln!(output, "=====================================");
    let _ = writeln!(output, "Errors:   {}", report.error_count());
    let _ = writeln!(output, "Warnings: {}", report.warning_count());

    if !report.conflicts.is_empty() {
        let _ = writeln!(output);
    }

    for conflict in &report.conflicts {
        let _ = writeln!(
            output,
            "[{}] {}: {}",
            severity_label(conflict.severity),
            conflict_kind_label(&conflict.kind),
            conflict.description
        );
        if let Some(suggestion) = &conflict.suggestion {
            let _ = writeln!(output, "  Suggestion: {suggestion}");
        }
        let _ = writeln!(output);
    }

    let _ = writeln!(output, "Exit code: {}", report.exit_code());
    output
}

fn format_report_github(report: &ConflictReport) -> String {
    let mut output = String::new();
    let _ = writeln!(output, "# Conflict Check");
    let _ = writeln!(output);
    let _ = writeln!(
        output,
        "**Branches**: `{}` vs `{}`",
        report.branch_a, report.branch_b
    );
    let _ = writeln!(output, "**Merge base**: `{}`", report.merge_base);
    let _ = writeln!(output);
    let _ = writeln!(output, "| Severity | Count |");
    let _ = writeln!(output, "|----------|-------|");
    let _ = writeln!(output, "| Error | {} |", report.error_count());
    let _ = writeln!(output, "| Warning | {} |", report.warning_count());

    if !report.conflicts.is_empty() {
        let _ = writeln!(output);
        let _ = writeln!(output, "## Conflicts");
        let _ = writeln!(output, "| # | Severity | Kind | Description |");
        let _ = writeln!(output, "|---|----------|------|-------------|");
        for (index, conflict) in report.conflicts.iter().enumerate() {
            let _ = writeln!(
                output,
                "| {} | {} | {} | {} |",
                index + 1,
                severity_title(conflict.severity),
                conflict_kind_label(&conflict.kind),
                conflict.description
            );
        }
    }

    output
}

pub fn format_check_hook_install_result(result: &CheckHookInstallResult, format: &str) -> String {
    match format {
        "json" => serde_json::to_string_pretty(result)
            .unwrap_or_else(|err| format!("{{\"error\": \"{err}\"}}")),
        _ => {
            let mut output = String::new();
            let _ = writeln!(output, "Semantic check hook");
            let _ = writeln!(output, "===================");
            let _ = writeln!(output, "Hook: {}", result.hook_name);
            let _ = writeln!(output, "Path: {}", result.hook_path.display());
            let _ = writeln!(output, "Status: {}", check_hook_status_label(result.status));
            let _ = writeln!(
                output,
                "Hooks path: {}",
                if result.uses_custom_hooks_path {
                    "custom"
                } else {
                    "default"
                }
            );
            let _ = writeln!(output);
            let _ = writeln!(
                output,
                "This hook runs `nex check HEAD MERGE_HEAD` before merge commit."
            );
            output
        }
    }
}

#[derive(Debug, Clone)]
struct HtmlMetric<'a> {
    label: &'a str,
    value: String,
    tone: &'a str,
}

fn format_demo_report_html(report: &DemoReport) -> String {
    let status_tone = if report.warnings.is_empty() {
        "positive"
    } else {
        "warning"
    };
    let status_label = if report.current_diff.available {
        "Snapshot ready"
    } else {
        "Snapshot partial"
    };
    let status_summary = if report.current_diff.available {
        "This repository can already be mapped semantically, including a live diff preview from the requested refs."
    } else {
        "The repository indexed semantically, but the default diff preview could not be built from the requested refs."
    };
    let metrics = vec![
        HtmlMetric {
            label: "Languages",
            value: if report.detected_languages.is_empty() {
                "0".to_string()
            } else {
                report.detected_languages.len().to_string()
            },
            tone: "neutral",
        },
        HtmlMetric {
            label: "Indexed files",
            value: report.indexed_files.to_string(),
            tone: "neutral",
        },
        HtmlMetric {
            label: "Semantic units",
            value: report.semantic_units.to_string(),
            tone: "neutral",
        },
        HtmlMetric {
            label: "Dependency edges",
            value: report.dependency_edges.to_string(),
            tone: status_tone,
        },
    ];

    let mut overview = String::new();
    overview.push_str("<section class=\"section\">");
    overview.push_str("<span class=\"section-kicker\">Repository snapshot</span>");
    overview.push_str("<h2>What Nexum Graph can see right now</h2>");
    let _ = writeln!(
        overview,
        "<p>Repo: <strong>{repo}</strong><br>Head: <strong>{head}</strong><br>Diff base: <strong>{base}</strong></p>",
        repo = escape_html(&report.repo_path.display().to_string()),
        head = escape_html(&report.head_commit),
        base = escape_html(&report.base_ref),
    );
    let _ = writeln!(
        overview,
        "<div class=\"keyline\"><strong>Languages detected:</strong> {}</div>",
        escape_html(&if report.detected_languages.is_empty() {
            "none".to_string()
        } else {
            report.detected_languages.join(", ")
        })
    );
    overview.push_str("</section>");

    let mut diff_preview = String::new();
    diff_preview.push_str("<section class=\"section\">");
    diff_preview.push_str("<span class=\"section-kicker\">Diff preview</span>");
    diff_preview.push_str("<h2>Current semantic change surface</h2>");
    if report.current_diff.available {
        let _ = writeln!(
            diff_preview,
            "<div class=\"card-grid\"><article class=\"item-card tone-positive\"><div class=\"item-head\"><span class=\"badge positive\">Added</span><span class=\"meta\">{}</span></div><h3>{}</h3></article><article class=\"item-card tone-warning\"><div class=\"item-head\"><span class=\"badge warning\">Modified</span><span class=\"meta\">{}</span></div><h3>{}</h3></article><article class=\"item-card tone-neutral\"><div class=\"item-head\"><span class=\"badge neutral\">Removed</span><span class=\"meta\">{}</span></div><h3>{}</h3></article><article class=\"item-card tone-neutral\"><div class=\"item-head\"><span class=\"badge neutral\">Moved</span><span class=\"meta\">{}</span></div><h3>{}</h3></article></div>",
            report.current_diff.added,
            report.current_diff.added,
            report.current_diff.modified,
            report.current_diff.modified,
            report.current_diff.removed,
            report.current_diff.removed,
            report.current_diff.moved,
            report.current_diff.moved,
        );
        if !report.current_diff.highlights.is_empty() {
            diff_preview.push_str("<div class=\"bullet-list\">");
            for highlight in &report.current_diff.highlights {
                let _ = writeln!(diff_preview, "<div>{}</div>", escape_html(highlight));
            }
            diff_preview.push_str("</div>");
        }
    } else if let Some(reason) = &report.current_diff.unavailable_reason {
        let _ = writeln!(
            diff_preview,
            "<div class=\"item-card tone-warning\"><div class=\"item-head\"><span class=\"badge warning\">Unavailable</span><span class=\"meta\">default refs</span></div><h3>Diff preview needs manual refs</h3><p>{}</p></div>",
            escape_html(reason)
        );
    }
    diff_preview.push_str("</section>");

    let mut workspace_state = String::new();
    workspace_state.push_str("<section class=\"section\">");
    workspace_state.push_str("<span class=\"section-kicker\">Operator state</span>");
    workspace_state.push_str("<h2>Live coordination footprint</h2>");
    let _ = writeln!(
        workspace_state,
        "<p>Active locks: <strong>{}</strong><br>Event log entries: <strong>{}</strong><br>Server auth: <strong>{}</strong></p>",
        report.active_locks,
        report.event_count,
        if report.auth_configured {
            "configured"
        } else {
            "not configured"
        },
    );
    if !report.warnings.is_empty() {
        workspace_state.push_str("<div class=\"bullet-list warning-list\">");
        for warning in &report.warnings {
            let _ = writeln!(workspace_state, "<div>{}</div>", escape_html(warning));
        }
        workspace_state.push_str("</div>");
    }
    workspace_state.push_str("</section>");

    let mut next_moves = String::new();
    next_moves.push_str("<section class=\"section\">");
    next_moves.push_str("<span class=\"section-kicker\">Next moves</span>");
    next_moves.push_str("<h2>What to run next</h2>");
    next_moves.push_str("<div class=\"card-grid\">");
    let _ = writeln!(
        next_moves,
        "<article class=\"item-card tone-positive\"><div class=\"item-head\"><span class=\"badge positive\">Guide</span><span class=\"meta\">recommended</span></div><h3>Run the guided setup</h3><p>Turn this raw snapshot into an operator path with merge-hook and auth guidance.</p><code class=\"command\">nex start --base {base} --head {head}</code></article>",
        base = escape_html(&report.base_ref),
        head = escape_html(&report.head_ref),
    );
    let _ = writeln!(
        next_moves,
        "<article class=\"item-card tone-neutral\"><div class=\"item-head\"><span class=\"badge neutral\">Inspect</span><span class=\"meta\">semantic diff</span></div><h3>Open the current diff</h3><p>See the exact semantic units behind this preview instead of only the summary counts.</p><code class=\"command\">nex diff {base} {head}</code></article>",
        base = escape_html(&report.base_ref),
        head = escape_html(&report.head_ref),
    );
    let _ = writeln!(
        next_moves,
        "<article class=\"item-card tone-warning\"><div class=\"item-head\"><span class=\"badge warning\">Protect</span><span class=\"meta\">local merges</span></div><h3>Install the semantic merge guard</h3><p>Make future merges run the branch conflict check automatically before merge commit.</p><code class=\"command\">nex check --install-hook</code></article>",
    );
    next_moves.push_str("</div></section>");

    render_html_shell(
        "Nexum Graph Demo Report",
        &report.repo_path.display().to_string(),
        &format!(
            "<span class=\"badge {tone}\">{label}</span><span class=\"score\">{files} files · {units} semantic units · {edges} dependency edges</span><p class=\"hero-copy\">{summary}</p>",
            tone = status_tone,
            label = escape_html(status_label),
            files = report.indexed_files,
            units = report.semantic_units,
            edges = report.dependency_edges,
            summary = escape_html(status_summary),
        ),
        &metrics,
        &[overview, diff_preview, workspace_state, next_moves],
    )
}

fn format_report_html(report: &ConflictReport) -> String {
    let (risk_label, risk_tone, risk_score, risk_summary) = merge_risk_summary(report);
    let metrics = vec![
        HtmlMetric {
            label: "Merge base",
            value: escape_html(&report.merge_base),
            tone: "neutral",
        },
        HtmlMetric {
            label: "Conflicts",
            value: report.conflicts.len().to_string(),
            tone: risk_tone,
        },
        HtmlMetric {
            label: "Errors",
            value: report.error_count().to_string(),
            tone: if report.error_count() > 0 {
                "critical"
            } else {
                "neutral"
            },
        },
        HtmlMetric {
            label: "Warnings",
            value: report.warning_count().to_string(),
            tone: if report.warning_count() > 0 {
                "warning"
            } else {
                "neutral"
            },
        },
    ];

    let mut overview = String::new();
    overview.push_str("<section class=\"section\">");
    overview.push_str("<span class=\"section-kicker\">Semantic merge view</span>");
    let _ = writeln!(
        overview,
        "<h2>{}</h2>",
        escape_html(&format!("{} vs {}", report.branch_a, report.branch_b))
    );
    let _ = writeln!(overview, "<p>{}</p>", escape_html(&risk_summary));
    overview.push_str(
        "<div class=\"keyline\"><strong>What this means:</strong> Nexum Graph is comparing branch-level semantic diffs, not just text patches.</div>",
    );
    overview.push_str("</section>");

    let mut conflicts = String::new();
    conflicts.push_str("<section class=\"section\">");
    conflicts.push_str("<span class=\"section-kicker\">Conflict breakdown</span>");
    conflicts.push_str("<h2>What would actually collide</h2>");
    if report.conflicts.is_empty() {
        conflicts.push_str(
            "<div class=\"item-card tone-positive\"><div class=\"item-head\"><span class=\"badge positive\">Clean</span><span class=\"meta\">No semantic blockers</span></div><h3>No blocking semantic conflicts detected.</h3><p>This branch pair is currently a good candidate for merge. Keep the semantic hook and PR check turned on so the state stays that way.</p></div>",
        );
    } else {
        conflicts.push_str("<div class=\"card-grid\">");
        for conflict in &report.conflicts {
            let _ = writeln!(
                conflicts,
                "<article class=\"item-card tone-{tone}\"><div class=\"item-head\"><span class=\"badge {tone}\">{severity}</span><span class=\"meta\">{kind}</span></div><h3>{description}</h3><p><strong>{branch_a}:</strong> {unit_a}<br><strong>{branch_b}:</strong> {unit_b}</p>{suggestion}</article>",
                tone = severity_tone(conflict.severity),
                severity = escape_html(severity_title(conflict.severity)),
                kind = escape_html(conflict_kind_label(&conflict.kind)),
                description = escape_html(&conflict.description),
                branch_a = escape_html(&report.branch_a),
                branch_b = escape_html(&report.branch_b),
                unit_a = escape_html(&conflict.unit_a.qualified_name),
                unit_b = escape_html(&conflict.unit_b.qualified_name),
                suggestion = conflict
                    .suggestion
                    .as_ref()
                    .map(|suggestion| format!(
                        "<p class=\"suggestion\"><strong>Suggested move:</strong> {}</p>",
                        escape_html(suggestion)
                    ))
                    .unwrap_or_default(),
            );
        }
        conflicts.push_str("</div>");
    }
    conflicts.push_str("</section>");

    render_html_shell(
        "Nexum Graph Semantic Check",
        &format!("{} vs {}", report.branch_a, report.branch_b),
        &format!(
            "<span class=\"badge {tone}\">{label}</span><span class=\"score\">Merge risk score {score}/100</span><p class=\"hero-copy\">{summary}</p>",
            tone = risk_tone,
            label = escape_html(&risk_label),
            score = risk_score,
            summary = escape_html(&risk_summary),
        ),
        &metrics,
        &[overview, conflicts],
    )
}

fn format_start_report_html(report: &StartReport) -> String {
    let (readiness_label, readiness_tone, readiness_score, readiness_summary) =
        start_readiness_summary(report);
    let metrics = vec![
        HtmlMetric {
            label: "Languages",
            value: if report.demo.detected_languages.is_empty() {
                "0".to_string()
            } else {
                report.demo.detected_languages.len().to_string()
            },
            tone: "neutral",
        },
        HtmlMetric {
            label: "Indexed files",
            value: report.demo.indexed_files.to_string(),
            tone: "neutral",
        },
        HtmlMetric {
            label: "Semantic units",
            value: report.demo.semantic_units.to_string(),
            tone: "neutral",
        },
        HtmlMetric {
            label: "Activation score",
            value: format!("{readiness_score}/100"),
            tone: readiness_tone,
        },
    ];

    let mut next_steps = String::new();
    next_steps.push_str("<section class=\"section\">");
    next_steps.push_str("<span class=\"section-kicker\">Operator path</span>");
    next_steps.push_str("<h2>What to do next</h2>");
    next_steps.push_str("<div class=\"card-grid\">");
    for (index, step) in report.next_steps.iter().enumerate() {
        let tone = start_step_tone(step.status);
        let _ = writeln!(
            next_steps,
            "<article class=\"item-card tone-{tone}\"><div class=\"item-head\"><span class=\"badge {tone}\">{status}</span><span class=\"meta\">Step {index}</span></div><h3>{title}</h3><p>{reason}</p><code class=\"command\">{command}</code></article>",
            tone = tone,
            status = escape_html(start_step_status_label(step.status)),
            index = index + 1,
            title = escape_html(&step.title),
            reason = escape_html(&step.reason),
            command = escape_html(&step.command),
        );
    }
    next_steps.push_str("</div></section>");

    let mut snapshot = String::new();
    snapshot.push_str("<section class=\"section\">");
    snapshot.push_str("<span class=\"section-kicker\">Current snapshot</span>");
    snapshot.push_str("<h2>What Nexum Graph already sees</h2>");
    let _ = writeln!(
        snapshot,
        "<p>{}</p>",
        escape_html(&format!(
            "Detected languages: {}. Dependency edges: {}. Active locks: {}. Event log entries: {}.",
            if report.demo.detected_languages.is_empty() {
                "none".to_string()
            } else {
                report.demo.detected_languages.join(", ")
            },
            report.demo.dependency_edges,
            report.demo.active_locks,
            report.demo.event_count
        ))
    );
    if report.demo.current_diff.available {
        let _ = writeln!(
            snapshot,
            "<div class=\"keyline\"><strong>Diff preview:</strong> {} added, {} modified, {} removed, {} moved.</div>",
            report.demo.current_diff.added,
            report.demo.current_diff.modified,
            report.demo.current_diff.removed,
            report.demo.current_diff.moved
        );
    } else if let Some(reason) = &report.demo.current_diff.unavailable_reason {
        let _ = writeln!(
            snapshot,
            "<div class=\"keyline\"><strong>Diff preview unavailable:</strong> {}</div>",
            escape_html(reason)
        );
    }
    if !report.demo.current_diff.highlights.is_empty() {
        snapshot.push_str("<div class=\"bullet-list\">");
        for highlight in &report.demo.current_diff.highlights {
            let _ = writeln!(snapshot, "<div>{}</div>", escape_html(highlight));
        }
        snapshot.push_str("</div>");
    }
    if !report.demo.warnings.is_empty() {
        snapshot.push_str("<div class=\"bullet-list warning-list\">");
        for warning in &report.demo.warnings {
            let _ = writeln!(snapshot, "<div>{}</div>", escape_html(warning));
        }
        snapshot.push_str("</div>");
    }
    snapshot.push_str("</section>");

    render_html_shell(
        "Nexum Graph Start Report",
        &report.repo_path.display().to_string(),
        &format!(
            "<span class=\"badge {tone}\">{label}</span><span class=\"score\">Activation score {score}/100</span><p class=\"hero-copy\">{summary}</p><p class=\"hero-copy\">Merge guard: <strong>{hook}</strong> at {hook_path}. Server auth: <strong>{auth}</strong>.</p>",
            tone = readiness_tone,
            label = escape_html(&readiness_label),
            score = readiness_score,
            summary = escape_html(&readiness_summary),
            hook = if report.hook_installed && report.hook_healthy {
                "ready"
            } else if report.hook_installed {
                "custom"
            } else {
                "not installed"
            },
            hook_path = escape_html(&report.hook_path.display().to_string()),
            auth = if report.auth_configured {
                "configured"
            } else {
                "not configured"
            },
        ),
        &metrics,
        &[next_steps, snapshot],
    )
}

fn render_html_shell(
    title: &str,
    subtitle: &str,
    hero_status_html: &str,
    metrics: &[HtmlMetric<'_>],
    sections: &[String],
) -> String {
    let mut output = String::new();
    output.push_str("<!DOCTYPE html><html lang=\"en\"><head><meta charset=\"utf-8\">");
    output.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">");
    let _ = writeln!(output, "<title>{}</title>", escape_html(title));
    output.push_str(
        r#"<style>
:root {
  color-scheme: light;
  --bg: #f6efe3;
  --ink: #10263a;
  --muted: #5c6b78;
  --panel: rgba(255, 255, 255, 0.82);
  --line: rgba(16, 38, 58, 0.12);
  --positive: #0f766e;
  --warning: #b45309;
  --critical: #b42318;
  --neutral: #334155;
  --shadow: 0 24px 60px rgba(16, 38, 58, 0.14);
}
* { box-sizing: border-box; }
body {
  margin: 0;
  min-height: 100vh;
  font-family: "Avenir Next", "Segoe UI", sans-serif;
  color: var(--ink);
  background:
    radial-gradient(circle at top left, rgba(217, 119, 6, 0.14), transparent 34rem),
    radial-gradient(circle at top right, rgba(15, 118, 110, 0.18), transparent 30rem),
    linear-gradient(180deg, #fbf7f1 0%, var(--bg) 100%);
}
.page {
  width: min(1120px, calc(100% - 32px));
  margin: 0 auto;
  padding: 40px 0 72px;
}
.hero {
  background: linear-gradient(135deg, rgba(16, 38, 58, 0.96), rgba(16, 90, 99, 0.94));
  color: #f8f4ed;
  border-radius: 30px;
  padding: 32px;
  box-shadow: var(--shadow);
}
.kicker,
.section-kicker {
  display: inline-block;
  font-size: 0.76rem;
  letter-spacing: 0.16em;
  text-transform: uppercase;
  color: rgba(248, 244, 237, 0.72);
}
.section-kicker { color: var(--muted); }
h1, h2, h3 {
  margin: 0;
  font-family: "Iowan Old Style", Georgia, serif;
  font-weight: 600;
}
h1 {
  margin-top: 10px;
  font-size: clamp(2.2rem, 4vw, 3.7rem);
  line-height: 1.02;
}
.subtitle {
  margin: 10px 0 0;
  font-size: 1rem;
  color: rgba(248, 244, 237, 0.76);
}
.hero-copy {
  margin: 14px 0 0;
  max-width: 64ch;
  line-height: 1.6;
  color: rgba(248, 244, 237, 0.88);
}
.hero-status {
  display: flex;
  flex-wrap: wrap;
  gap: 12px;
  align-items: center;
  margin-top: 20px;
}
.score {
  font-size: 0.92rem;
  color: rgba(248, 244, 237, 0.84);
}
.metrics,
.card-grid {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(200px, 1fr));
  gap: 16px;
}
.metrics { margin-top: 28px; }
.metric,
.section,
.item-card {
  background: var(--panel);
  border: 1px solid var(--line);
  border-radius: 22px;
  box-shadow: var(--shadow);
}
.metric {
  padding: 18px 20px;
  backdrop-filter: blur(10px);
}
.metric-label {
  font-size: 0.82rem;
  letter-spacing: 0.08em;
  text-transform: uppercase;
  color: var(--muted);
}
.metric-value {
  margin-top: 10px;
  font-size: 1.9rem;
  line-height: 1;
}
.metric.positive .metric-value { color: var(--positive); }
.metric.warning .metric-value { color: var(--warning); }
.metric.critical .metric-value { color: var(--critical); }
.section {
  margin-top: 22px;
  padding: 24px;
  backdrop-filter: blur(10px);
}
.section p {
  color: var(--muted);
  line-height: 1.65;
}
.keyline {
  margin-top: 14px;
  padding: 14px 16px;
  border-left: 4px solid rgba(16, 38, 58, 0.22);
  background: rgba(255, 255, 255, 0.5);
  border-radius: 14px;
}
.item-card {
  padding: 18px;
}
.item-head {
  display: flex;
  flex-wrap: wrap;
  justify-content: space-between;
  gap: 10px;
  align-items: center;
}
.badge {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 7px 12px;
  border-radius: 999px;
  font-size: 0.75rem;
  letter-spacing: 0.08em;
  text-transform: uppercase;
  font-weight: 600;
}
.badge.positive { background: rgba(15, 118, 110, 0.14); color: var(--positive); }
.badge.warning { background: rgba(180, 83, 9, 0.14); color: var(--warning); }
.badge.critical { background: rgba(180, 35, 24, 0.14); color: var(--critical); }
.badge.neutral { background: rgba(51, 65, 85, 0.12); color: var(--neutral); }
.meta {
  font-size: 0.85rem;
  color: var(--muted);
}
.item-card h3 {
  margin-top: 14px;
  font-size: 1.25rem;
}
.item-card p {
  margin-bottom: 0;
}
.suggestion {
  margin-top: 14px;
  color: var(--ink);
}
.command {
  display: block;
  margin-top: 16px;
  padding: 14px 16px;
  overflow-x: auto;
  border-radius: 16px;
  background: #132b3a;
  color: #f8f4ed;
  font-family: "IBM Plex Mono", "Consolas", monospace;
  font-size: 0.92rem;
}
.bullet-list {
  display: grid;
  gap: 10px;
  margin-top: 16px;
}
.bullet-list > div {
  padding: 12px 14px;
  border-radius: 14px;
  background: rgba(255, 255, 255, 0.52);
  border: 1px solid rgba(16, 38, 58, 0.08);
}
.warning-list > div {
  border-left: 4px solid rgba(180, 83, 9, 0.28);
}
.footer {
  margin-top: 18px;
  font-size: 0.86rem;
  color: var(--muted);
  text-align: center;
}
@media (max-width: 720px) {
  .page { width: min(100% - 20px, 1120px); padding-top: 20px; }
  .hero { padding: 24px; border-radius: 24px; }
  .section { padding: 20px; }
}
</style>"#,
    );
    output.push_str("</head><body><main class=\"page\"><header class=\"hero\">");
    output.push_str("<span class=\"kicker\">Nexum Graph</span>");
    let _ = writeln!(output, "<h1>{}</h1>", escape_html(title));
    let _ = writeln!(
        output,
        "<p class=\"subtitle\">{}</p>",
        escape_html(subtitle)
    );
    let _ = writeln!(
        output,
        "<div class=\"hero-status\">{hero_status_html}</div>"
    );
    output.push_str("<div class=\"metrics\">");
    for metric in metrics {
        let _ = writeln!(
            output,
            "<div class=\"metric {tone}\"><div class=\"metric-label\">{label}</div><div class=\"metric-value\">{value}</div></div>",
            tone = metric.tone,
            label = escape_html(metric.label),
            value = metric.value,
        );
    }
    output.push_str("</div></header>");
    for section in sections {
        output.push_str(section);
    }
    output.push_str("<p class=\"footer\">Generated by Nexum Graph. Semantic coordination for multi-agent software engineering.</p>");
    output.push_str("</main></body></html>");
    output
}

fn merge_risk_summary(report: &ConflictReport) -> (String, &'static str, usize, String) {
    let errors = report.error_count();
    let warnings = report.warning_count();
    let score = (errors.saturating_mul(45) + warnings.saturating_mul(18)).min(100);
    if errors > 0 {
        (
            "High merge risk".to_string(),
            "critical",
            score.max(80),
            format!(
                "{} blocking semantic error(s) detected across {} and {}.",
                errors, report.branch_a, report.branch_b
            ),
        )
    } else if warnings > 0 {
        (
            "Review recommended".to_string(),
            "warning",
            score.max(40),
            format!(
                "{} warning-level semantic conflict(s) detected. Merge is possible, but it is not clean.",
                warnings
            ),
        )
    } else {
        (
            "Clean semantic check".to_string(),
            "positive",
            0,
            format!(
                "No blocking semantic conflicts detected between {} and {}.",
                report.branch_a, report.branch_b
            ),
        )
    }
}

fn start_readiness_summary(report: &StartReport) -> (String, &'static str, usize, String) {
    let mut score = 0usize;
    if report.demo.indexed_files > 0 {
        score += 30;
    }
    if report.demo.current_diff.available {
        score += 25;
    }
    if report.hook_installed && report.hook_healthy {
        score += 25;
    }
    if report.auth_configured {
        score += 20;
    }

    if score >= 75 {
        (
            "Ready to coordinate".to_string(),
            "positive",
            score,
            "This repo already has enough Nexum Graph structure in place to show value immediately."
                .to_string(),
        )
    } else if score >= 45 {
        (
            "Halfway activated".to_string(),
            "warning",
            score,
            "The semantic graph is working. Install the merge guard and complete the operator path to make it durable."
                .to_string(),
        )
    } else {
        (
            "Just getting started".to_string(),
            "neutral",
            score,
            "The repo scan works, but the highest-value coordination safeguards still need to be turned on."
                .to_string(),
        )
    }
}

fn severity_tone(severity: Severity) -> &'static str {
    match severity {
        Severity::Info => "neutral",
        Severity::Warning => "warning",
        Severity::Error => "critical",
    }
}

fn start_step_tone(status: StartStepStatus) -> &'static str {
    match status {
        StartStepStatus::Recommended => "warning",
        StartStepStatus::Ready => "neutral",
        StartStepStatus::Complete => "positive",
    }
}

fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn severity_label(severity: Severity) -> &'static str {
    match severity {
        Severity::Info => "INFO",
        Severity::Warning => "WARNING",
        Severity::Error => "ERROR",
    }
}

fn severity_title(severity: Severity) -> &'static str {
    match severity {
        Severity::Info => "Info",
        Severity::Warning => "Warning",
        Severity::Error => "Error",
    }
}

fn conflict_kind_label(kind: &ConflictKind) -> &'static str {
    match kind {
        ConflictKind::BrokenReference { .. } => "BrokenReference",
        ConflictKind::ConcurrentBodyEdit { .. } => "ConcurrentBodyEdit",
        ConflictKind::SignatureMismatch { .. } => "SignatureMismatch",
        ConflictKind::DeletedDependency { .. } => "DeletedDependency",
        ConflictKind::NamingCollision { .. } => "NamingCollision",
        ConflictKind::InterfaceDrift { .. } => "InterfaceDrift",
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Phase 2: Coordination output
// ─────────────────────────────────────────────────────────────────────────────

use crate::coordination_pipeline::LockEntry;
use nex_core::SemanticUnit;
use nex_core::{LockResult, ValidationReport};
use nex_eventlog::{RollbackOutcome, SemanticEvent};

/// Format a `LockResult` for CLI output.
///
/// Supported formats: `"json"`, `"text"`.
/// Unknown formats fall back to `"text"`.
///
/// **Text format (Granted)**:
/// ```text
/// Lock GRANTED: {agent_name} -> {target_name}
/// ```
///
/// **Text format (Denied)**:
/// ```text
/// Lock DENIED: {agent_name} -> {target_name}
/// Conflicts:
///   - {conflict.reason}
///   - {conflict.reason}
/// ```
///
/// **JSON format**: Pretty-printed `LockResult` via serde_json.
pub fn format_lock_result(
    result: &LockResult,
    agent_name: &str,
    target_name: &str,
    format: &str,
) -> String {
    match format {
        "json" => serde_json::to_string_pretty(result)
            .unwrap_or_else(|err| format!("{{\"error\": \"{err}\"}}")),
        _ => match result {
            LockResult::Granted => format!("Lock GRANTED: {agent_name} -> {target_name}\n"),
            LockResult::Denied { conflicts } => {
                let mut output = String::new();
                let _ = writeln!(output, "Lock DENIED: {agent_name} -> {target_name}");
                let _ = writeln!(output, "Conflicts:");
                for conflict in conflicts {
                    let _ = writeln!(output, "  - {}", conflict.reason);
                }
                output
            }
        },
    }
}

/// Format a list of `LockEntry` items for CLI output.
///
/// Supported formats: `"json"`, `"text"`.
/// Unknown formats fall back to `"text"`.
///
/// **Text format (non-empty)**:
/// ```text
/// Active Locks (N)
/// ================
///   [Write]  alice -> processRequest
///   [Read]   bob   -> validate
/// ```
///
/// **Text format (empty)**:
/// ```text
/// Active Locks (0)
/// ================
///   (none)
/// ```
///
/// **JSON format**: Pretty-printed `Vec<LockEntry>` via serde_json.
///
/// Use `std::fmt::Write` for building the text output, consistent
/// with the existing formatters in this module.
/// Format the kind with `{:?}` (Debug formatting, e.g. `Write`, `Read`).
pub fn format_locks(entries: &[LockEntry], format: &str) -> String {
    match format {
        "json" => serde_json::to_string_pretty(entries)
            .unwrap_or_else(|err| format!("{{\"error\": \"{err}\"}}")),
        _ => {
            let mut output = String::new();
            let _ = writeln!(output, "Active Locks ({})", entries.len());
            let _ = writeln!(output, "================");
            if entries.is_empty() {
                let _ = writeln!(output, "  (none)");
            } else {
                for entry in entries {
                    let _ = writeln!(
                        output,
                        "  [{:?}] {} -> {}",
                        entry.kind, entry.agent_name, entry.target_name
                    );
                }
            }
            output
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Phase 2: Validation output
// ─────────────────────────────────────────────────────────────────────────────

/// Format a `ValidationReport` for CLI output.
///
/// Supported formats: `"json"`, `"text"`.
/// Unknown formats fall back to `"text"`.
///
/// **Text format (clean)**:
/// ```text
/// Validation: alice (3 units checked)
/// ====================================
/// Errors:   0
/// Warnings: 0
///
/// All checks passed.
/// ```
///
/// **Text format (issues)**:
/// ```text
/// Validation: alice (3 units checked)
/// ====================================
/// Errors:   1
/// Warnings: 1
///
/// [ERROR] UnlockedModification: modified `validate` without a Write lock
///   Suggestion: run `nex lock alice validate write` first
///
/// [WARNING] StaleCallers: `processRequest` may be using old signature of `validate`
///   Suggestion: update `processRequest` to match new signature of `validate`
///
/// Exit code: 1
/// ```
///
/// **JSON format**: Pretty-printed `ValidationReport` via serde_json.
///
/// Use `std::fmt::Write` for building text output.
/// Format issue severity with the existing `severity_label()` helper (uppercase).
/// Format the `ValidationKind` variant name with `validation_kind_label()`.
pub fn format_validation_report(report: &ValidationReport, format: &str) -> String {
    match format {
        "json" => serde_json::to_string_pretty(report)
            .unwrap_or_else(|err| format!("{{\"error\": \"{err}\"}}")),
        _ => {
            let mut output = String::new();
            let _ = writeln!(
                output,
                "Validation: {} ({} units checked)",
                report.agent_name, report.units_checked
            );
            let _ = writeln!(output, "====================================");
            let _ = writeln!(output, "Errors:   {}", report.error_count());
            let _ = writeln!(output, "Warnings: {}", report.warning_count());

            if report.issues.is_empty() {
                let _ = writeln!(output);
                let _ = writeln!(output, "All checks passed.");
            } else {
                let _ = writeln!(output);
                for issue in &report.issues {
                    let _ = writeln!(
                        output,
                        "[{}] {}: {}",
                        severity_label(issue.severity),
                        validation_kind_label(&issue.kind),
                        issue.description
                    );
                    if let Some(suggestion) = &issue.suggestion {
                        let _ = writeln!(output, "  Suggestion: {suggestion}");
                    }
                    let _ = writeln!(output);
                }
                let _ = writeln!(output, "Exit code: {}", report.exit_code());
            }

            output
        }
    }
}

fn validation_kind_label(kind: &nex_core::ValidationKind) -> &'static str {
    match kind {
        nex_core::ValidationKind::UnlockedModification { .. } => "UnlockedModification",
        nex_core::ValidationKind::UnlockedDeletion { .. } => "UnlockedDeletion",
        nex_core::ValidationKind::BrokenReference { .. } => "BrokenReference",
        nex_core::ValidationKind::StaleCallers { .. } => "StaleCallers",
    }
}

// Phase 3: Event log output

pub fn format_event_log(events: &[SemanticEvent], format: &str) -> String {
    match format {
        "json" => serde_json::to_string_pretty(events)
            .unwrap_or_else(|err| format!("{{\"error\": \"{err}\"}}")),
        _ => {
            let mut output = String::new();
            let _ = writeln!(output, "Semantic Event Log ({})", events.len());
            let _ = writeln!(output, "======================");

            if events.is_empty() {
                let _ = writeln!(output, "  (none)");
            } else {
                for event in events {
                    let _ = writeln!(
                        output,
                        "- [{}] {}",
                        event.timestamp.to_rfc3339(),
                        event.description
                    );
                    let _ = writeln!(output, "  Intent: {}", event.intent_id);
                    let _ = writeln!(output, "  Agent: {}", event.agent_id);
                    let _ = writeln!(output, "  Mutations: {}", event.mutations.len());
                    if !event.tags.is_empty() {
                        let _ = writeln!(output, "  Tags: {}", event.tags.join(", "));
                    }
                }
            }

            output
        }
    }
}

pub fn format_rollback_outcome(outcome: &RollbackOutcome, format: &str) -> String {
    match format {
        "json" => serde_json::to_string_pretty(outcome)
            .unwrap_or_else(|err| format!("{{\"error\": \"{err}\"}}")),
        _ => {
            let mut output = String::new();
            if let Some(event) = &outcome.rollback_event {
                let _ = writeln!(output, "Rollback APPLIED: {}", outcome.original_intent_id);
                let _ = writeln!(output, "Event: {}", event.id);
                let _ = writeln!(output, "Mutations: {}", event.mutations.len());
            } else {
                let _ = writeln!(output, "Rollback BLOCKED: {}", outcome.original_intent_id);
                let _ = writeln!(output, "Conflicts:");
                for conflict in &outcome.conflicts {
                    let _ = writeln!(output, "  - {}", conflict.reason);
                }
            }
            output
        }
    }
}

pub fn format_replay_state(units: &[SemanticUnit], format: &str) -> String {
    match format {
        "json" => serde_json::to_string_pretty(units)
            .unwrap_or_else(|err| format!("{{\"error\": \"{err}\"}}")),
        _ => {
            let mut output = String::new();
            let _ = writeln!(output, "Replayed State ({})", units.len());
            let _ = writeln!(output, "==================");

            if units.is_empty() {
                let _ = writeln!(output, "  (none)");
            } else {
                for unit in units {
                    let _ = writeln!(
                        output,
                        "  - {:?} {} ({})",
                        unit.kind,
                        unit.qualified_name,
                        unit.file_path.display()
                    );
                }
            }

            output
        }
    }
}

pub fn format_auth_init_result(result: &AuthInitResult, format: &str) -> String {
    match format {
        "json" => serde_json::to_string_pretty(result)
            .unwrap_or_else(|err| format!("{{\"error\": \"{err}\"}}")),
        _ => {
            let mut output = String::new();
            let _ = writeln!(output, "Auth config initialized: {}", result.path.display());
            let _ = writeln!(output, "Mode: {}", auth_mode_label(result.mode));
            let _ = writeln!(output, "Storage: hash-at-rest");
            if result.replaced_existing {
                let _ = writeln!(output, "Replaced existing config: yes");
            }
            let _ = writeln!(output, "Issued tokens:");
            for issued in &result.issued {
                match &issued.agent_name {
                    Some(agent_name) => {
                        let _ = writeln!(output, "  {}: {}", agent_name, issued.token);
                    }
                    None => {
                        let _ = writeln!(output, "  shared: {}", issued.token);
                    }
                }
            }
            let _ = writeln!(output, "Raw tokens are shown only once.");
            output
        }
    }
}

pub fn format_auth_issue_result(result: &AuthIssueResult, format: &str) -> String {
    match format {
        "json" => serde_json::to_string_pretty(result)
            .unwrap_or_else(|err| format!("{{\"error\": \"{err}\"}}")),
        _ => {
            let mut output = String::new();
            match &result.issued.agent_name {
                Some(agent_name) => {
                    let _ = writeln!(output, "Issued agent token for {agent_name}");
                }
                None => {
                    let _ = writeln!(output, "Issued shared bearer token");
                }
            }
            let _ = writeln!(output, "Path: {}", result.path.display());
            let _ = writeln!(output, "Mode: {}", auth_mode_label(result.mode));
            let _ = writeln!(output, "Storage: hash-at-rest");
            let _ = writeln!(output, "Token: {}", result.issued.token);
            let _ = writeln!(output, "Active tokens: {}", result.active_token_count);
            let _ = writeln!(output, "Revoked tokens: {}", result.revoked_token_count);
            let _ = writeln!(output, "Raw token is shown only once.");
            output
        }
    }
}

pub fn format_auth_revoke_result(result: &AuthRevokeResult, format: &str) -> String {
    match format {
        "json" => serde_json::to_string_pretty(result)
            .unwrap_or_else(|err| format!("{{\"error\": \"{err}\"}}")),
        _ => {
            let mut output = String::new();
            if result.removed {
                let _ = writeln!(output, "Revoked token: {}", result.token);
            } else {
                let _ = writeln!(output, "Token already revoked: {}", result.token);
            }
            let _ = writeln!(output, "Path: {}", result.path.display());
            let _ = writeln!(output, "Mode: {}", auth_mode_label(result.mode));
            let _ = writeln!(output, "Storage: hash-at-rest");
            if let Some(agent_name) = &result.affected_agent {
                let _ = writeln!(output, "Affected agent: {agent_name}");
            }
            let _ = writeln!(output, "Active tokens: {}", result.active_token_count);
            let _ = writeln!(output, "Revoked tokens: {}", result.revoked_token_count);
            output
        }
    }
}

pub fn format_auth_status(status: &AuthStatus, format: &str) -> String {
    match format {
        "json" => serde_json::to_string_pretty(status)
            .unwrap_or_else(|err| format!("{{\"error\": \"{err}\"}}")),
        _ => {
            let mut output = String::new();
            let _ = writeln!(output, "Auth config: {}", status.path.display());
            let _ = writeln!(
                output,
                "Status: {}",
                if status.exists {
                    "configured"
                } else {
                    "not configured"
                }
            );
            if status.using_backup {
                let _ = writeln!(output, "Source: backup");
            }
            let _ = writeln!(output, "Mode: {}", auth_mode_label(status.mode));
            let _ = writeln!(output, "Storage: hash-at-rest");
            let _ = writeln!(output, "Shared tokens: {}", status.shared_token_count);
            let _ = writeln!(output, "Revoked tokens: {}", status.revoked_token_count);
            let _ = writeln!(output, "Agents: {}", status.agents.len());
            for agent in &status.agents {
                let _ = writeln!(
                    output,
                    "  {}: {} {}",
                    agent.agent_name,
                    agent.active_tokens,
                    pluralize("active token", agent.active_tokens)
                );
            }
            output
        }
    }
}

pub fn format_audit_verification_report(report: &AuditVerificationReport, format: &str) -> String {
    match format {
        "json" => serde_json::to_string_pretty(report)
            .unwrap_or_else(|err| format!("{{\"error\": \"{err}\"}}")),
        _ => {
            let mut output = String::new();
            let _ = writeln!(output, "Audit Verification");
            let _ = writeln!(output, "==================");
            let _ = writeln!(output, "Log: {}", report.log_path.display());
            let _ = writeln!(output, "Head: {}", report.head_path.display());
            let _ = writeln!(
                output,
                "Status: {}",
                if report.valid { "VALID" } else { "INVALID" }
            );
            let _ = writeln!(output, "Anchored: {}", yes_no(report.anchored));
            let _ = writeln!(output, "Records: {}", report.record_count);
            if let Some(last_hash) = &report.last_hash {
                let _ = writeln!(output, "Last hash: {last_hash}");
            }
            if report.issues.is_empty() {
                let _ = writeln!(output);
                let _ = writeln!(output, "No integrity issues detected.");
            } else {
                let _ = writeln!(output);
                let _ = writeln!(output, "Issues:");
                for issue in &report.issues {
                    match issue.line {
                        Some(line) => {
                            let _ = writeln!(
                                output,
                                "  - [{}] line {}: {}",
                                issue.kind, line, issue.description
                            );
                        }
                        None => {
                            let _ = writeln!(output, "  - [{}] {}", issue.kind, issue.description);
                        }
                    }
                }
                let _ = writeln!(output);
                let _ = writeln!(output, "Exit code: {}", report.exit_code());
            }
            output
        }
    }
}

fn auth_mode_label(mode: AuthConfigMode) -> &'static str {
    match mode {
        AuthConfigMode::Disabled => "disabled",
        AuthConfigMode::Shared => "shared",
        AuthConfigMode::Agent => "per-agent",
    }
}

fn pluralize(label: &str, count: usize) -> &str {
    if count == 1 {
        label
    } else {
        match label {
            "active token" => "active tokens",
            _ => label,
        }
    }
}

fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}

fn check_hook_status_label(status: CheckHookInstallStatus) -> &'static str {
    match status {
        CheckHookInstallStatus::Installed => "installed",
        CheckHookInstallStatus::Updated => "updated",
        CheckHookInstallStatus::Unchanged => "already current",
    }
}

fn start_step_status_label(status: StartStepStatus) -> &'static str {
    match status {
        StartStepStatus::Recommended => "recommended",
        StartStepStatus::Ready => "ready",
        StartStepStatus::Complete => "complete",
    }
}

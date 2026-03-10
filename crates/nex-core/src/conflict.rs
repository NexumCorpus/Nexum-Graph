//! Authoritative conflict detection types for Phase 1.
//!
//! These types are transcribed from the Implementation Specification §Phase 1.
//! They define the output contract for `nex check` — the semantic conflict
//! detection engine. Codex must not alter these type signatures.

use crate::{SemanticId, SemanticUnit};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

// ─────────────────────────────────────────────────────────────────────────────
// Semantic Conflict (the output of conflict detection)
// ─────────────────────────────────────────────────────────────────────────────

/// A semantic conflict detected between two branches.
///
/// Produced by `ConflictDetector::detect()` when cross-referencing
/// two semantic diffs (base→A, base→B) reveals incompatible changes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticConflict {
    /// What kind of conflict this is.
    pub kind: ConflictKind,
    /// How severe the conflict is.
    pub severity: Severity,
    /// The unit from branch A involved in the conflict.
    pub unit_a: SemanticUnit,
    /// The unit from branch B involved in the conflict.
    pub unit_b: SemanticUnit,
    /// Human-readable description of the conflict.
    pub description: String,
    /// Optional suggestion for resolution.
    pub suggestion: Option<String>,
}

/// Classification of semantic conflicts.
///
/// Each variant captures the specific IDs involved, enabling precise
/// error messages and automated fix suggestions.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ConflictKind {
    /// Branch A renamed/removed a function that branch B still calls.
    BrokenReference {
        caller: SemanticId,
        callee: SemanticId,
    },
    /// Both branches modify the same function body.
    ConcurrentBodyEdit { unit: SemanticId },
    /// Branch A changed a function's signature, but branch B calls it
    /// expecting the old signature.
    SignatureMismatch {
        function: SemanticId,
        caller: SemanticId,
    },
    /// Branch A deleted a unit that branch B depends on.
    DeletedDependency {
        deleted: SemanticId,
        dependent: SemanticId,
    },
    /// Both branches introduce a unit with the same qualified name.
    NamingCollision { name: String },
    /// Branch A changed an interface, but branch B's implementor
    /// doesn't conform to the new shape.
    InterfaceDrift {
        interface_id: SemanticId,
        implementor: SemanticId,
    },
}

/// Severity levels for semantic conflicts.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// Informational only — no action required.
    Info,
    /// May cause issues — manual review recommended.
    Warning,
    /// Will break at compile/runtime — must resolve before merge.
    Error,
}

// ─────────────────────────────────────────────────────────────────────────────
// Conflict Report (aggregated output)
// ─────────────────────────────────────────────────────────────────────────────

/// Aggregated result of conflict detection.
///
/// Exit codes: 0 = clean, 1 = errors found, 2 = warnings only.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConflictReport {
    /// All detected conflicts, ordered by severity (errors first).
    pub conflicts: Vec<SemanticConflict>,
    /// Branch A ref string.
    pub branch_a: String,
    /// Branch B ref string.
    pub branch_b: String,
    /// Merge base commit (hex string).
    pub merge_base: String,
}

impl ConflictReport {
    /// Count of Error-severity conflicts.
    pub fn error_count(&self) -> usize {
        self.conflicts
            .iter()
            .filter(|c| c.severity == Severity::Error)
            .count()
    }

    /// Count of Warning-severity conflicts.
    pub fn warning_count(&self) -> usize {
        self.conflicts
            .iter()
            .filter(|c| c.severity == Severity::Warning)
            .count()
    }

    /// Suggested exit code: 0 = clean, 1 = errors, 2 = warnings only.
    pub fn exit_code(&self) -> i32 {
        if self.error_count() > 0 {
            1
        } else if self.warning_count() > 0 {
            2
        } else {
            0
        }
    }

    /// Deterministic merge-risk score for CLI and report rendering.
    pub fn risk_score(&self) -> usize {
        let errors = self.error_count();
        let warnings = self.warning_count();
        let infos = self
            .conflicts
            .len()
            .saturating_sub(errors.saturating_add(warnings));
        let mut score = errors.saturating_mul(45) + warnings.saturating_mul(18) + infos * 8;
        if self.conflicts.len() >= 4 {
            score = score.saturating_add(10);
        }
        if errors > 0 {
            score = score.max(80);
        } else if warnings > 0 {
            score = score.max(40);
        }
        score.min(100)
    }

    /// Human-readable risk label aligned to the current report contents.
    pub fn risk_label(&self) -> &'static str {
        if self.error_count() > 0 {
            "High merge risk"
        } else if self.warning_count() > 0 {
            "Review recommended"
        } else {
            "Clean semantic check"
        }
    }

    /// One-line risk summary for hero copy and PR comments.
    pub fn risk_summary(&self) -> String {
        let errors = self.error_count();
        let warnings = self.warning_count();
        if errors > 0 {
            format!(
                "{} blocking semantic error(s) detected across {} and {}.",
                errors, self.branch_a, self.branch_b
            )
        } else if warnings > 0 {
            format!(
                "{} warning-level semantic conflict(s) detected. Merge is possible, but it is not clean.",
                warnings
            )
        } else {
            format!(
                "No blocking semantic conflicts detected between {} and {}.",
                self.branch_a, self.branch_b
            )
        }
    }

    /// Short reasons explaining why the report scored the way it did.
    pub fn risk_reasons(&self) -> Vec<String> {
        if self.conflicts.is_empty() {
            return vec!["No blocking semantic conflicts detected.".to_string()];
        }

        let mut reasons = Vec::new();
        let errors = self.error_count();
        let warnings = self.warning_count();
        if errors > 0 {
            reasons.push(format!(
                "{} blocking semantic error(s) must be resolved before merge.",
                errors
            ));
        } else if warnings > 0 {
            reasons.push(format!(
                "{} warning-level semantic conflict(s) still need review.",
                warnings
            ));
        }

        let mut grouped: BTreeMap<&'static str, (usize, u8, String)> = BTreeMap::new();
        for conflict in &self.conflicts {
            let (message, priority) = conflict_kind_risk_message(&conflict.kind);
            let example = conflict_focus_name(conflict);
            let entry = grouped
                .entry(message)
                .or_insert_with(|| (0usize, priority, example.clone()));
            entry.0 += 1;
            if entry.2.is_empty() && !example.is_empty() {
                entry.2 = example;
            }
        }

        let mut grouped = grouped
            .into_iter()
            .map(|(message, (count, priority, example))| (count, priority, message, example))
            .collect::<Vec<_>>();
        grouped.sort_by(|left, right| {
            right
                .0
                .cmp(&left.0)
                .then_with(|| left.1.cmp(&right.1))
                .then_with(|| left.2.cmp(right.2))
        });

        for (count, _, message, example) in grouped {
            let detail = if example.is_empty() {
                if count == 1 {
                    format!("{message}.")
                } else {
                    format!("{message} ({count} cases).")
                }
            } else if count == 1 {
                format!("{message}, including `{example}`.")
            } else {
                format!("{message} ({count} cases), including `{example}`.")
            };
            reasons.push(detail);
            if reasons.len() == 3 {
                break;
            }
        }

        reasons
    }

    /// Deterministic, user-facing next actions for resolving the current report.
    pub fn recommended_actions(&self) -> Vec<String> {
        if self.conflicts.is_empty() {
            return vec![
                "Merge is a good candidate; keep the semantic hook and PR check enabled."
                    .to_string(),
            ];
        }

        let mut actions = Vec::new();
        if self.error_count() > 0 {
            actions.push("Resolve blocking semantic conflicts before merge.".to_string());
        } else if self.warning_count() > 0 {
            actions.push("Review warning-level semantic conflicts before merge.".to_string());
        }

        let kinds = self
            .conflicts
            .iter()
            .map(|conflict| recommended_action_key(&conflict.kind))
            .collect::<BTreeSet<_>>();
        for key in kinds {
            actions.push(match key {
                RecommendedActionKey::RestoreDependents => {
                    "Update or restore callers and dependents that still point at removed units."
                        .to_string()
                }
                RecommendedActionKey::UpdateCallers => {
                    "Rebase the caller branch and update call sites to the current signature."
                        .to_string()
                }
                RecommendedActionKey::ReconcileBodyEdits => {
                    "Manually reconcile concurrent edits on the same semantic unit.".to_string()
                }
                RecommendedActionKey::RenameUnits => {
                    "Rename or consolidate duplicate qualified names before merge.".to_string()
                }
                RecommendedActionKey::SyncImplementors => {
                    "Sync implementors with the updated interface contract.".to_string()
                }
            });
        }

        actions.push(format!(
            "Rerun `nex check {} {}` after rebasing or reconciling the branches.",
            self.branch_a, self.branch_b
        ));
        actions
    }
}

fn conflict_kind_risk_message(kind: &ConflictKind) -> (&'static str, u8) {
    match kind {
        ConflictKind::DeletedDependency { .. } => (
            "One branch removed units that the other branch still depends on",
            0,
        ),
        ConflictKind::BrokenReference { .. } => (
            "Callers on one branch still point at renamed or removed units",
            1,
        ),
        ConflictKind::SignatureMismatch { .. } => (
            "Callers on one branch expect outdated function signatures",
            2,
        ),
        ConflictKind::ConcurrentBodyEdit { .. } => {
            ("Both branches changed the same semantic unit body", 3)
        }
        ConflictKind::InterfaceDrift { .. } => {
            ("Interface changes and implementor updates drifted apart", 4)
        }
        ConflictKind::NamingCollision { .. } => {
            ("Both branches introduced the same qualified name", 5)
        }
    }
}

fn conflict_focus_name(conflict: &SemanticConflict) -> String {
    match &conflict.kind {
        ConflictKind::NamingCollision { name } => name.clone(),
        _ if !conflict.unit_a.qualified_name.is_empty() => conflict.unit_a.qualified_name.clone(),
        _ => conflict.unit_b.qualified_name.clone(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum RecommendedActionKey {
    RestoreDependents,
    UpdateCallers,
    ReconcileBodyEdits,
    RenameUnits,
    SyncImplementors,
}

fn recommended_action_key(kind: &ConflictKind) -> RecommendedActionKey {
    match kind {
        ConflictKind::BrokenReference { .. } | ConflictKind::DeletedDependency { .. } => {
            RecommendedActionKey::RestoreDependents
        }
        ConflictKind::SignatureMismatch { .. } => RecommendedActionKey::UpdateCallers,
        ConflictKind::ConcurrentBodyEdit { .. } => RecommendedActionKey::ReconcileBodyEdits,
        ConflictKind::NamingCollision { .. } => RecommendedActionKey::RenameUnits,
        ConflictKind::InterfaceDrift { .. } => RecommendedActionKey::SyncImplementors,
    }
}

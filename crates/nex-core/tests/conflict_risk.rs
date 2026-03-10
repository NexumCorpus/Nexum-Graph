use nex_core::{ConflictKind, ConflictReport, SemanticConflict, SemanticUnit, Severity, UnitKind};
use std::path::PathBuf;

fn sample_unit(id_byte: u8, qualified_name: &str) -> SemanticUnit {
    SemanticUnit {
        id: [id_byte; 32],
        kind: UnitKind::Function,
        name: qualified_name
            .rsplit("::")
            .next()
            .expect("unit name")
            .to_string(),
        qualified_name: qualified_name.to_string(),
        file_path: PathBuf::from("src/lib.rs"),
        byte_range: 0..32,
        signature_hash: u64::from(id_byte),
        body_hash: u64::from(id_byte) + 1,
        dependencies: vec![],
    }
}

#[test]
fn clean_report_has_zero_risk() {
    let report = ConflictReport {
        conflicts: vec![],
        branch_a: "main".into(),
        branch_b: "feature/clean".into(),
        merge_base: "abc123".into(),
    };

    assert_eq!(report.risk_score(), 0);
    assert_eq!(report.risk_label(), "Clean semantic check");
    assert_eq!(
        report.risk_reasons(),
        vec!["No blocking semantic conflicts detected.".to_string()]
    );
}

#[test]
fn risk_reasons_group_conflicts_deterministically() {
    let report = ConflictReport {
        conflicts: vec![
            SemanticConflict {
                kind: ConflictKind::ConcurrentBodyEdit { unit: [1; 32] },
                severity: Severity::Error,
                unit_a: sample_unit(1, "auth::validate"),
                unit_b: sample_unit(2, "auth::validate"),
                description: "Concurrent edit".into(),
                suggestion: None,
            },
            SemanticConflict {
                kind: ConflictKind::DeletedDependency {
                    deleted: [3; 32],
                    dependent: [4; 32],
                },
                severity: Severity::Warning,
                unit_a: sample_unit(3, "billing::charge"),
                unit_b: sample_unit(4, "api::submit_order"),
                description: "Deleted dependency".into(),
                suggestion: None,
            },
            SemanticConflict {
                kind: ConflictKind::DeletedDependency {
                    deleted: [5; 32],
                    dependent: [6; 32],
                },
                severity: Severity::Warning,
                unit_a: sample_unit(5, "inventory::reserve"),
                unit_b: sample_unit(6, "api::submit_order"),
                description: "Another deleted dependency".into(),
                suggestion: None,
            },
        ],
        branch_a: "main".into(),
        branch_b: "feature/orders".into(),
        merge_base: "abc123".into(),
    };

    let reasons = report.risk_reasons();

    assert_eq!(report.risk_score(), 81);
    assert_eq!(report.risk_label(), "High merge risk");
    assert_eq!(
        reasons[0],
        "1 blocking semantic error(s) must be resolved before merge."
    );
    assert_eq!(
        reasons[1],
        "One branch removed units that the other branch still depends on (2 cases), including `billing::charge`."
    );
    assert_eq!(
        reasons[2],
        "Both branches changed the same semantic unit body, including `auth::validate`."
    );
}

#[test]
fn recommended_actions_are_deterministic_and_specific() {
    let report = ConflictReport {
        conflicts: vec![
            SemanticConflict {
                kind: ConflictKind::DeletedDependency {
                    deleted: [3; 32],
                    dependent: [4; 32],
                },
                severity: Severity::Error,
                unit_a: sample_unit(3, "billing::charge"),
                unit_b: sample_unit(4, "api::submit_order"),
                description: "Deleted dependency".into(),
                suggestion: None,
            },
            SemanticConflict {
                kind: ConflictKind::SignatureMismatch {
                    function: [5; 32],
                    caller: [6; 32],
                },
                severity: Severity::Warning,
                unit_a: sample_unit(5, "auth::validate"),
                unit_b: sample_unit(6, "api::submit_order"),
                description: "Signature mismatch".into(),
                suggestion: None,
            },
        ],
        branch_a: "main".into(),
        branch_b: "feature/orders".into(),
        merge_base: "abc123".into(),
    };

    assert_eq!(
        report.recommended_actions(),
        vec![
            "Resolve blocking semantic conflicts before merge.".to_string(),
            "Update or restore callers and dependents that still point at removed units."
                .to_string(),
            "Rebase the caller branch and update call sites to the current signature.".to_string(),
            "Rerun `nex check main feature/orders` after rebasing or reconciling the branches."
                .to_string(),
        ]
    );
}

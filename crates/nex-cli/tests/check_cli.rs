use clap::Parser;
use nex_cli::check_pipeline::{
    CheckHookInstallStatus, install_check_hook, merge_check_hook_script,
};
use nex_cli::cli::{Cli, Commands};
use nex_cli::output::format_report;
use nex_core::{ConflictKind, ConflictReport, SemanticConflict, SemanticUnit, Severity, UnitKind};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;

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

fn init_temp_repo() -> (tempfile::TempDir, git2::Repository) {
    let dir = tempfile::tempdir().expect("create tempdir");
    let repo = git2::Repository::init(dir.path()).expect("init repo");

    let mut config = repo.config().expect("get config");
    config.set_str("user.name", "Test").expect("set name");
    config
        .set_str("user.email", "test@example.com")
        .expect("set email");

    (dir, repo)
}

fn write_and_stage(repo: &git2::Repository, relative_path: &str, content: &str) {
    let full_path = repo.workdir().unwrap().join(relative_path);
    if let Some(parent) = full_path.parent() {
        std::fs::create_dir_all(parent).expect("create dirs");
    }
    std::fs::write(&full_path, content).expect("write file");

    let mut index = repo.index().expect("get index");
    index
        .add_path(Path::new(relative_path))
        .expect("add to index");
    index.write().expect("write index");
}

fn commit(repo: &git2::Repository, msg: &str) {
    let mut index = repo.index().expect("get index");
    let tree_oid = index.write_tree().expect("write tree");
    let tree = repo.find_tree(tree_oid).expect("find tree");
    let sig = repo.signature().expect("sig");
    let parents: Vec<git2::Commit> = match repo.head() {
        Ok(head) => vec![head.peel_to_commit().expect("peel")],
        Err(_) => vec![],
    };
    let refs: Vec<&git2::Commit> = parents.iter().collect();
    repo.commit(Some("HEAD"), &sig, &sig, msg, &tree, &refs)
        .expect("commit");
}

fn setup_repo() -> (tempfile::TempDir, git2::Repository) {
    let (dir, repo) = init_temp_repo();
    write_and_stage(
        &repo,
        "handler.ts",
        r#"function validate(input: string): boolean { return input.length > 0; }
function processRequest(req: string): void { validate(req); }
"#,
    );
    commit(&repo, "initial");
    (dir, repo)
}

#[test]
fn check_command_parses_refs() {
    let cli =
        Cli::try_parse_from(["nex", "check", "feature/a", "feature/b"]).expect("parse check refs");

    match cli.command {
        Commands::Check {
            branch_a,
            branch_b,
            install_hook,
            ..
        } => {
            assert_eq!(branch_a.as_deref(), Some("feature/a"));
            assert_eq!(branch_b.as_deref(), Some("feature/b"));
            assert!(!install_hook);
        }
        other => panic!("expected check command, got {other:?}"),
    }
}

#[test]
fn check_command_parses_install_hook_without_refs() {
    let cli = Cli::try_parse_from(["nex", "check", "--install-hook"]).expect("parse install hook");

    match cli.command {
        Commands::Check {
            branch_a,
            branch_b,
            install_hook,
            force,
            ..
        } => {
            assert!(branch_a.is_none());
            assert!(branch_b.is_none());
            assert!(install_hook);
            assert!(!force);
        }
        other => panic!("expected check command, got {other:?}"),
    }
}

#[test]
fn install_check_hook_writes_default_commit_message_hook() {
    let (_dir, repo) = setup_repo();
    let repo_path = repo.workdir().unwrap();

    let result = install_check_hook(repo_path, false).expect("install hook");
    let hook_path = repo.path().join("hooks").join("commit-msg");

    assert_eq!(result.hook_path, hook_path);
    assert_eq!(result.status, CheckHookInstallStatus::Installed);
    assert!(!result.uses_custom_hooks_path);
    assert_eq!(
        std::fs::read_to_string(&hook_path).expect("read hook"),
        merge_check_hook_script()
    );
}

#[test]
fn install_check_hook_uses_custom_hooks_path_when_configured() {
    let (_dir, repo) = setup_repo();
    let repo_path = repo.workdir().unwrap();
    repo.config()
        .expect("get config")
        .set_str("core.hooksPath", ".githooks")
        .expect("set hooks path");

    let result = install_check_hook(repo_path, false).expect("install hook");
    let hook_path = repo_path.join(".githooks").join("commit-msg");

    assert_eq!(result.hook_path, hook_path);
    assert!(result.uses_custom_hooks_path);
    assert_eq!(result.status, CheckHookInstallStatus::Installed);
}

#[test]
fn install_check_hook_requires_force_to_replace_existing_script() {
    let (_dir, repo) = setup_repo();
    let repo_path = repo.workdir().unwrap();
    let hook_path = repo.path().join("hooks").join("commit-msg");
    std::fs::create_dir_all(hook_path.parent().expect("hook dir")).expect("create hook dir");
    std::fs::write(&hook_path, "#!/usr/bin/env sh\necho custom\n").expect("write custom hook");

    let error = install_check_hook(repo_path, false).expect_err("expected replace error");
    assert!(
        error.to_string().contains("pass --force to replace"),
        "unexpected error: {error}"
    );

    let forced = install_check_hook(repo_path, true).expect("force install hook");
    assert_eq!(forced.status, CheckHookInstallStatus::Updated);
    assert_eq!(
        std::fs::read_to_string(&hook_path).expect("read updated hook"),
        merge_check_hook_script()
    );
}

#[test]
fn install_check_hook_is_idempotent_when_current_script_is_present() {
    let (_dir, repo) = setup_repo();
    let repo_path = repo.workdir().unwrap();

    let first = install_check_hook(repo_path, false).expect("install hook");
    let second = install_check_hook(repo_path, false).expect("reinstall hook");

    assert_eq!(first.status, CheckHookInstallStatus::Installed);
    assert_eq!(second.status, CheckHookInstallStatus::Unchanged);
}

#[test]
fn installed_hook_gates_real_merge_commits_in_linked_worktree() {
    let (dir, repo) = setup_repo();
    let workdir = dir.path().join("linked");
    let output = Command::new("git")
        .current_dir(repo.workdir().expect("fixture workdir"))
        .args(["worktree", "add", "--detach"])
        .arg(&workdir)
        .output()
        .expect("create a linked worktree");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let base = repo.head().unwrap().target().unwrap().to_string();
    let bin_dir = Path::new(env!("CARGO_BIN_EXE_nex")).parent().unwrap();
    let path = std::env::var_os("PATH").expect("PATH");
    let path = std::env::join_paths(
        std::iter::once(bin_dir.to_owned()).chain(std::env::split_paths(&path)),
    )
    .expect("test PATH");
    let git = |args: &[&str]| {
        Command::new("git")
            .current_dir(&workdir)
            .env("PATH", &path)
            .args(args)
            .output()
            .expect("run git")
    };
    let git_ok = |args: &[&str]| {
        let output = git(args);
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap().trim().to_owned()
    };
    let write_commit = |file: &str, source: &str, message: &str| {
        std::fs::write(workdir.join(file), source).expect("write fixture source");
        git_ok(&["add", "--", file]);
        git_ok(&["commit", "-m", message]);
    };

    git_ok(&["config", "commit.gpgsign", "false"]);
    git_ok(&["config", "core.autocrlf", "false"]);
    git_ok(&["switch", "-c", "change-signature"]);
    write_commit(
        "handler.ts",
        r#"function validate(input: string, strict: boolean): boolean { return strict && input.length > 0; }
function processRequest(req: string): void { validate(req, true); }
"#,
        "update signature and existing caller",
    );
    git_ok(&["switch", "-c", "add-stale-call", &base]);
    write_commit(
        "report.ts",
        r#"function report(): boolean { return validate("row"); }"#,
        "add stale caller",
    );
    git_ok(&["switch", "-c", "add-banner", &base]);
    write_commit(
        "banner.ts",
        r#"function banner() { return "hello"; }"#,
        "add independent banner",
    );
    git_ok(&["switch", "change-signature"]);
    let hook = install_check_hook(&workdir, false).expect("install merge check");
    assert_eq!(hook.hook_path, repo.path().join("hooks").join("commit-msg"));

    let ordinary = git(&["commit", "--allow-empty", "-m", "ordinary commit"]);
    assert!(ordinary.status.success());
    assert!(!String::from_utf8_lossy(&ordinary.stderr).contains("nex check hook:"));
    let before_merge = git_ok(&["rev-parse", "HEAD"]);
    let check_blocked = |args: &[&str]| {
        let output = git(args);
        assert!(!output.status.success());
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stdout.contains("SignatureMismatch") || stderr.contains("SignatureMismatch"),
            "git {args:?}: {stdout}\n{stderr}"
        );
        assert_eq!(git_ok(&["rev-parse", "HEAD"]), before_merge);
        git_ok(&["merge", "--abort"]);
    };
    check_blocked(&["merge", "--no-ff", "--no-edit", "add-stale-call"]);
    git_ok(&["merge", "--no-ff", "--no-commit", "add-stale-call"]);
    check_blocked(&["commit", "--no-edit"]);

    let clean = git(&["merge", "--no-ff", "--no-edit", "add-banner"]);
    let stdout = String::from_utf8_lossy(&clean.stdout);
    let stderr = String::from_utf8_lossy(&clean.stderr);
    assert!(clean.status.success(), "clean merge: {stdout}\n{stderr}");
    assert!(
        stdout.contains("No blocking semantic conflicts")
            || stderr.contains("No blocking semantic conflicts")
    );
    assert_ne!(git_ok(&["rev-parse", "HEAD"]), before_merge);
    assert_eq!(
        git_ok(&["rev-parse", "HEAD^2"]),
        git_ok(&["rev-parse", "add-banner"])
    );
}

#[test]
fn format_check_text_report_includes_merge_risk_reasons() {
    let report = ConflictReport {
        conflicts: vec![
            SemanticConflict {
                kind: ConflictKind::ConcurrentBodyEdit { unit: [1; 32] },
                severity: Severity::Error,
                unit_a: sample_unit(1, "auth::validate"),
                unit_b: sample_unit(2, "auth::validate"),
                description: "Both branches modified auth::validate in incompatible ways.".into(),
                suggestion: Some("Merge the intended validation behavior before landing.".into()),
            },
            SemanticConflict {
                kind: ConflictKind::DeletedDependency {
                    deleted: [3; 32],
                    dependent: [4; 32],
                },
                severity: Severity::Warning,
                unit_a: sample_unit(3, "billing::charge"),
                unit_b: sample_unit(4, "api::submit_order"),
                description: "feature/orders still depends on billing::charge.".into(),
                suggestion: Some("Retarget submit_order before merging.".into()),
            },
        ],
        branch_a: "main".into(),
        branch_b: "feature/orders".into(),
        merge_base: "abc123".into(),
    };

    let text = format_report(&report, "text");

    assert!(text.contains("Merge risk: High merge risk (80/100)"));
    assert!(text.contains("Why this scored this way:"));
    assert!(text.contains("Recommended next actions:"));
    assert!(text.contains("1 blocking semantic error(s) must be resolved before merge."));
    assert!(text.contains("One branch removed units that the other branch still depends on"));
    assert!(text.contains("Both branches changed the same semantic unit body"));
    assert!(text.contains("Manually reconcile concurrent edits on the same semantic unit."));
}

#[test]
fn format_check_github_report_includes_merge_risk_reasons() {
    let report = ConflictReport {
        conflicts: vec![SemanticConflict {
            kind: ConflictKind::DeletedDependency {
                deleted: [3; 32],
                dependent: [4; 32],
            },
            severity: Severity::Warning,
            unit_a: sample_unit(3, "billing::charge"),
            unit_b: sample_unit(4, "api::submit_order"),
            description: "feature/orders still depends on billing::charge.".into(),
            suggestion: Some("Retarget submit_order before merging.".into()),
        }],
        branch_a: "main".into(),
        branch_b: "feature/orders".into(),
        merge_base: "abc123".into(),
    };

    let github = format_report(&report, "github");

    assert!(github.contains("**Merge risk**: **Review recommended** (40/100)"));
    assert!(github.contains("## Why this scored this way"));
    assert!(github.contains("## Recommended next actions"));
    assert!(github.contains("- 1 warning-level semantic conflict(s) still need review."));
    assert!(github.contains("- One branch removed units that the other branch still depends on"));
    assert!(
        github.contains(
            "- Update or restore callers and dependents that still point at removed units."
        )
    );
}

#[test]
fn format_check_insights_json_includes_structured_risk_fields() {
    let report = ConflictReport {
        conflicts: vec![SemanticConflict {
            kind: ConflictKind::DeletedDependency {
                deleted: [3; 32],
                dependent: [4; 32],
            },
            severity: Severity::Warning,
            unit_a: sample_unit(3, "billing::charge"),
            unit_b: sample_unit(4, "api::submit_order"),
            description: "feature/orders still depends on billing::charge.".into(),
            suggestion: Some("Retarget submit_order before merging.".into()),
        }],
        branch_a: "main".into(),
        branch_b: "feature/orders".into(),
        merge_base: "abc123".into(),
    };

    let payload: Value =
        serde_json::from_str(&format_report(&report, "insights-json")).expect("parse insights");

    assert_eq!(payload["merge_risk"]["label"], "Review recommended");
    assert_eq!(payload["merge_risk"]["score"], 40);
    assert_eq!(payload["merge_risk"]["warning_conflicts"], 1);
    assert_eq!(
        payload["recommended_actions"][1],
        "Update or restore callers and dependents that still point at removed units."
    );
    assert_eq!(payload["report"]["branch_a"], "main");
}

#[test]
fn format_check_sarif_includes_results_and_rule_ids() {
    let report = ConflictReport {
        conflicts: vec![SemanticConflict {
            kind: ConflictKind::DeletedDependency {
                deleted: [3; 32],
                dependent: [4; 32],
            },
            severity: Severity::Warning,
            unit_a: sample_unit(3, "billing::charge"),
            unit_b: sample_unit(4, "api::submit_order"),
            description: "feature/orders still depends on billing::charge.".into(),
            suggestion: Some("Retarget submit_order before merging.".into()),
        }],
        branch_a: "main".into(),
        branch_b: "feature/orders".into(),
        merge_base: "abc123".into(),
    };

    let payload: Value =
        serde_json::from_str(&format_report(&report, "sarif")).expect("parse sarif");

    assert_eq!(payload["version"], "2.1.0");
    assert_eq!(payload["runs"][0]["tool"]["driver"]["name"], "Nexum Graph");
    assert_eq!(
        payload["runs"][0]["results"][0]["ruleId"],
        "nex.deleted-dependency"
    );
    assert_eq!(payload["runs"][0]["results"][0]["level"], "warning");
    assert_eq!(
        payload["runs"][0]["results"][0]["locations"][0]["physicalLocation"]["artifactLocation"]["uri"],
        "src/lib.rs"
    );
}

#[test]
fn format_check_html_report_has_visual_sections() {
    let report = ConflictReport {
        conflicts: vec![SemanticConflict {
            kind: ConflictKind::ConcurrentBodyEdit { unit: [1; 32] },
            severity: Severity::Error,
            unit_a: sample_unit(1, "auth::validate"),
            unit_b: sample_unit(2, "auth::validate"),
            description: "Both branches modified auth::validate in incompatible ways.".into(),
            suggestion: Some("Merge the intended validation behavior before landing.".into()),
        }],
        branch_a: "main".into(),
        branch_b: "feature/risk".into(),
        merge_base: "abc123".into(),
    };

    let html = format_report(&report, "html");

    assert!(html.starts_with("<!DOCTYPE html>"));
    assert!(html.contains("Nexum Graph Semantic Check"));
    assert!(html.contains("Merge risk score"));
    assert!(html.contains("How to unblock this merge"));
    assert!(html.contains("Both branches modified auth::validate in incompatible ways."));
    assert!(html.contains("Suggested move"));
}

#[test]
fn format_check_html_report_handles_clean_merge() {
    let report = ConflictReport {
        conflicts: vec![],
        branch_a: "main".into(),
        branch_b: "feature/clean".into(),
        merge_base: "def456".into(),
    };

    let html = format_report(&report, "html");

    assert!(html.contains("No blocking semantic conflicts detected."));
    assert!(html.contains("good candidate for merge"));
}

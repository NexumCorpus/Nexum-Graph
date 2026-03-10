use clap::Parser;
use nex_cli::cli::{Cli, Commands, GithubCommands};
use nex_cli::github_pipeline::{
    GitHubWorkflowRolloutStage, assess_github_status, default_workflow_path, gate_mode_satisfies,
    review_surfaces_enabled, run_github_init, run_github_status, verify_github_status,
};
use nex_cli::output::{format_github_init_result, format_github_status};
use std::fs;

fn current_release() -> String {
    format!("v{}", env!("CARGO_PKG_VERSION"))
}

fn init_repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    git2::Repository::init(dir.path()).expect("init repo");
    dir
}

#[test]
fn github_init_command_parses_gate_mode() {
    let cli = Cli::try_parse_from([
        "nex",
        "github",
        "init",
        "--gate-mode",
        "advisory",
        "--no-pr-comment",
        "--format",
        "json",
    ])
    .expect("parse github init");

    match cli.command {
        Commands::Github {
            command:
                GithubCommands::Init {
                    gate_mode,
                    no_pr_comment,
                    format,
                    ..
                },
        } => {
            assert_eq!(gate_mode, "advisory");
            assert!(no_pr_comment);
            assert_eq!(format, "json");
        }
        other => panic!("expected github init, got {other:?}"),
    }
}

#[test]
fn github_status_command_parses_format() {
    let cli = Cli::try_parse_from([
        "nex",
        "github",
        "status",
        "--format",
        "json",
        "--require-current",
        "--min-gate-mode",
        "errors-only",
        "--require-pr-comment",
        "--require-sarif",
    ])
    .expect("parse github status");

    match cli.command {
        Commands::Github {
            command:
                GithubCommands::Status {
                    format,
                    require_current,
                    min_gate_mode,
                    require_pr_comment,
                    require_sarif,
                    ..
                },
        } => {
            assert_eq!(format, "json");
            assert!(require_current);
            assert_eq!(min_gate_mode.as_deref(), Some("errors-only"));
            assert!(require_pr_comment);
            assert!(require_sarif);
        }
        other => panic!("expected github status, got {other:?}"),
    }
}

#[test]
fn gate_mode_satisfies_orders_rollout_modes() {
    assert!(gate_mode_satisfies(Some("strict"), "errors-only"));
    assert!(gate_mode_satisfies(Some("errors-only"), "errors-only"));
    assert!(!gate_mode_satisfies(Some("advisory"), "errors-only"));
    assert!(!gate_mode_satisfies(None, "errors-only"));
    assert!(!gate_mode_satisfies(Some("strict"), "unknown"));
}

#[test]
fn review_surfaces_enabled_requires_pr_comment_and_sarif() {
    let dir = init_repo();
    run_github_init(dir.path(), "Semantic Check", "strict", true, true, false)
        .expect("write workflow");
    let enabled = run_github_status(dir.path()).expect("status");
    assert!(review_surfaces_enabled(&enabled));

    let workflow_path = default_workflow_path(dir.path());
    fs::write(
        &workflow_path,
        format!(
            "name: Semantic Check\non:\n  pull_request:\n\njobs:\n  semantic-check:\n    uses: NexumCorpus/Nexum-Graph/.github/workflows/reusable-semantic-check.yml@{}\n    with:\n      format: github\n      gate-mode: strict\n      post-pr-comment: false\n      upload-sarif: true\n",
            current_release()
        ),
    )
    .expect("write limited workflow");
    let limited = run_github_status(dir.path()).expect("limited status");
    assert!(!review_surfaces_enabled(&limited));
}

#[test]
fn assess_github_status_classifies_rollout_posture() {
    let dir = init_repo();

    let missing = run_github_status(dir.path()).expect("missing status");
    let missing_assessment = assess_github_status(&missing);
    assert_eq!(
        missing_assessment.rollout_stage,
        GitHubWorkflowRolloutStage::Missing
    );
    assert_eq!(
        missing_assessment.recommended_command.as_deref(),
        Some("nex github init --gate-mode errors-only")
    );

    run_github_init(dir.path(), "Semantic Check", "strict", true, true, false)
        .expect("write workflow");
    let ready = run_github_status(dir.path()).expect("ready status");
    let ready_assessment = assess_github_status(&ready);
    assert_eq!(
        ready_assessment.rollout_stage,
        GitHubWorkflowRolloutStage::Ready
    );
    assert!(ready_assessment.branch_protection_ready);
    assert_eq!(ready_assessment.recommended_command, None);
}

#[test]
fn run_github_init_writes_reusable_workflow_file() {
    let dir = init_repo();

    let result = run_github_init(
        dir.path(),
        "Semantic Check",
        "errors-only",
        true,
        true,
        false,
    )
    .expect("write workflow");
    let workflow_path = default_workflow_path(dir.path());
    let workflow = fs::read_to_string(&workflow_path).expect("read workflow");

    assert_eq!(result.workflow_path, workflow_path);
    assert!(workflow.contains(
        "uses: NexumCorpus/Nexum-Graph/.github/workflows/reusable-semantic-check.yml@v0.1.0"
    ));
    assert!(workflow.contains("gate-mode: errors-only"));
    assert!(workflow.contains("post-pr-comment: true"));
    assert!(workflow.contains("upload-sarif: true"));
}

#[test]
fn run_github_status_detects_managed_workflow() {
    let dir = init_repo();
    run_github_init(
        dir.path(),
        "Semantic Check",
        "errors-only",
        true,
        true,
        false,
    )
    .expect("write workflow");

    let status = run_github_status(dir.path()).expect("status");

    assert!(status.exists);
    assert!(status.managed_by_nexum_graph);
    assert_eq!(
        status.workflow_ref.as_deref(),
        Some(current_release().as_str())
    );
    assert_eq!(status.current_ref, current_release());
    assert_eq!(status.up_to_date, Some(true));
    assert_eq!(status.gate_mode.as_deref(), Some("errors-only"));
    assert_eq!(status.post_pr_comment, Some(true));
    assert_eq!(status.upload_sarif, Some(true));
}

#[test]
fn run_github_status_detects_outdated_managed_workflow() {
    let dir = init_repo();
    let workflow_path = default_workflow_path(dir.path());
    fs::create_dir_all(workflow_path.parent().expect("workflow dir")).expect("mkdirs");
    fs::write(
        &workflow_path,
        "name: Semantic Check\non:\n  pull_request:\n\njobs:\n  semantic-check:\n    uses: NexumCorpus/Nexum-Graph/.github/workflows/reusable-semantic-check.yml@v0.0.9\n    with:\n      format: github\n      gate-mode: advisory\n      post-pr-comment: false\n      upload-sarif: false\n",
    )
    .expect("write outdated workflow");

    let status = run_github_status(dir.path()).expect("status");

    assert!(status.exists);
    assert!(status.managed_by_nexum_graph);
    assert_eq!(status.workflow_ref.as_deref(), Some("v0.0.9"));
    assert_eq!(status.current_ref, current_release());
    assert_eq!(status.up_to_date, Some(false));
    assert_eq!(status.gate_mode.as_deref(), Some("advisory"));
}

#[test]
fn run_github_status_detects_custom_workflow() {
    let dir = init_repo();
    let workflow_path = default_workflow_path(dir.path());
    fs::create_dir_all(workflow_path.parent().expect("workflow dir")).expect("mkdirs");
    fs::write(
        &workflow_path,
        "name: Custom Check\non:\n  pull_request:\njobs:\n  lint:\n    runs-on: ubuntu-latest\n",
    )
    .expect("write custom workflow");

    let status = run_github_status(dir.path()).expect("status");

    assert!(status.exists);
    assert!(!status.managed_by_nexum_graph);
    assert_eq!(status.workflow_ref, None);
    assert_eq!(status.up_to_date, None);
    assert_eq!(status.workflow_name.as_deref(), Some("Custom Check"));
    assert_eq!(status.gate_mode, None);
}

#[test]
fn run_github_init_requires_force_to_replace_existing_workflow() {
    let dir = init_repo();

    run_github_init(
        dir.path(),
        "Semantic Check",
        "errors-only",
        true,
        true,
        false,
    )
    .expect("write workflow");
    let error = run_github_init(dir.path(), "Semantic Check", "strict", true, true, false)
        .expect_err("expected replace error");
    assert!(
        error.to_string().contains("pass --force to replace"),
        "unexpected error: {error}"
    );

    let forced = run_github_init(dir.path(), "Semantic Check", "strict", false, false, true)
        .expect("force replace workflow");
    let workflow =
        fs::read_to_string(default_workflow_path(dir.path())).expect("read replaced workflow");

    assert!(forced.replaced_existing);
    assert!(workflow.contains("gate-mode: strict"));
    assert!(workflow.contains("post-pr-comment: false"));
    assert!(workflow.contains("upload-sarif: false"));
}

#[test]
fn format_github_init_result_reports_next_steps() {
    let dir = init_repo();
    let result = run_github_init(
        dir.path(),
        "Semantic Check",
        "errors-only",
        true,
        true,
        false,
    )
    .expect("write workflow");

    let text = format_github_init_result(&result, "text");

    assert!(text.contains("GitHub semantic check workflow"));
    assert!(text.contains("Gate mode: errors-only"));
    assert!(text.contains("git commit -m \"Add Nexum Graph semantic PR gate\""));
}

#[test]
fn format_github_status_reports_install_state() {
    let dir = init_repo();

    let missing = format_github_status(
        &run_github_status(dir.path()).expect("missing status"),
        "text",
    );
    assert!(missing.contains("Status: not installed"));
    assert!(missing.contains("nex github init --gate-mode errors-only"));

    run_github_init(dir.path(), "Semantic Check", "strict", false, false, false)
        .expect("write workflow");
    let managed = format_github_status(
        &run_github_status(dir.path()).expect("managed status"),
        "text",
    );
    assert!(managed.contains("Status: managed by Nexum Graph"));
    assert!(managed.contains("Gate mode: strict"));
    assert!(managed.contains(&format!("Pinned release: {}", current_release())));
}

#[test]
fn format_github_status_reports_outdated_workflow() {
    let dir = init_repo();
    let workflow_path = default_workflow_path(dir.path());
    fs::create_dir_all(workflow_path.parent().expect("workflow dir")).expect("mkdirs");
    fs::write(
        &workflow_path,
        "name: Semantic Check\non:\n  pull_request:\n\njobs:\n  semantic-check:\n    uses: NexumCorpus/Nexum-Graph/.github/workflows/reusable-semantic-check.yml@v0.0.9\n    with:\n      format: github\n      gate-mode: advisory\n      post-pr-comment: false\n      upload-sarif: false\n",
    )
    .expect("write outdated workflow");

    let status = run_github_status(dir.path()).expect("status");
    let text = format_github_status(&status, "text");

    assert!(text.contains("Status: managed by Nexum Graph (update available)"));
    assert!(text.contains("Pinned release: v0.0.9"));
    assert!(text.contains(&format!("Current release: {}", current_release())));
    assert!(text.contains("nex github init --gate-mode errors-only --force"));
}

#[test]
fn format_github_status_reports_limited_review_surfaces() {
    let dir = init_repo();
    let workflow_path = default_workflow_path(dir.path());
    fs::create_dir_all(workflow_path.parent().expect("workflow dir")).expect("mkdirs");
    fs::write(
        &workflow_path,
        format!(
            "name: Semantic Check\non:\n  pull_request:\n\njobs:\n  semantic-check:\n    uses: NexumCorpus/Nexum-Graph/.github/workflows/reusable-semantic-check.yml@{}\n    with:\n      format: github\n      gate-mode: strict\n      post-pr-comment: false\n      upload-sarif: true\n",
            current_release()
        ),
    )
    .expect("write limited workflow");

    let status = run_github_status(dir.path()).expect("status");
    let text = format_github_status(&status, "text");

    assert!(text.contains("Status: managed by Nexum Graph (review surfaces limited)"));
    assert!(text.contains("Rollout posture: partial review surface"));
    assert!(text.contains("PR comment: disabled"));
    assert!(text.contains("nex github init --gate-mode strict --force"));
}

#[test]
fn format_github_status_json_includes_assessment() {
    let dir = init_repo();
    run_github_init(dir.path(), "Semantic Check", "strict", true, true, false)
        .expect("write workflow");

    let status = run_github_status(dir.path()).expect("status");
    let json = format_github_status(&status, "json");

    assert!(json.contains("\"assessment\""));
    assert!(json.contains("\"rollout_stage\": \"ready\""));
    assert!(json.contains("\"branch_protection_ready\": true"));
}

#[test]
fn verify_github_status_accepts_current_managed_workflow() {
    let dir = init_repo();
    run_github_init(dir.path(), "Semantic Check", "strict", true, true, false)
        .expect("write workflow");

    let status = run_github_status(dir.path()).expect("status");
    verify_github_status(&status, true, false, None, false, false)
        .expect("managed workflow should pass");
    verify_github_status(&status, false, true, None, false, false)
        .expect("current workflow should pass");
    verify_github_status(&status, false, false, Some("errors-only"), true, true)
        .expect("strict workflow should satisfy errors-only minimum");
}

#[test]
fn verify_github_status_rejects_missing_custom_and_outdated_workflows() {
    let dir = init_repo();

    let missing = run_github_status(dir.path()).expect("missing status");
    let missing_error = verify_github_status(&missing, true, false, None, false, false)
        .expect_err("missing workflow should fail");
    assert!(missing_error.to_string().contains("not installed"));

    let workflow_path = default_workflow_path(dir.path());
    fs::create_dir_all(workflow_path.parent().expect("workflow dir")).expect("mkdirs");
    fs::write(
        &workflow_path,
        "name: Custom Check\non:\n  pull_request:\njobs:\n  lint:\n    runs-on: ubuntu-latest\n",
    )
    .expect("write custom workflow");
    let custom = run_github_status(dir.path()).expect("custom status");
    let custom_error = verify_github_status(&custom, true, false, None, false, false)
        .expect_err("custom workflow should fail");
    assert!(custom_error.to_string().contains("custom"));

    fs::write(
        &workflow_path,
        "name: Semantic Check\non:\n  pull_request:\n\njobs:\n  semantic-check:\n    uses: NexumCorpus/Nexum-Graph/.github/workflows/reusable-semantic-check.yml@v0.0.9\n    with:\n      format: github\n      gate-mode: advisory\n      post-pr-comment: false\n      upload-sarif: false\n",
    )
    .expect("write outdated workflow");
    let outdated = run_github_status(dir.path()).expect("outdated status");
    let outdated_error = verify_github_status(&outdated, false, true, None, false, false)
        .expect_err("outdated workflow should fail");
    assert!(outdated_error.to_string().contains("pinned to v0.0.9"));

    let advisory_error =
        verify_github_status(&outdated, false, false, Some("errors-only"), false, false)
            .expect_err("advisory workflow should fail minimum gate requirement");
    assert!(
        advisory_error
            .to_string()
            .contains("below required `errors-only`")
    );

    let workflow_path = default_workflow_path(dir.path());
    fs::write(
        &workflow_path,
        format!(
            "name: Semantic Check\non:\n  pull_request:\n\njobs:\n  semantic-check:\n    uses: NexumCorpus/Nexum-Graph/.github/workflows/reusable-semantic-check.yml@{}\n    with:\n      format: github\n      gate-mode: strict\n      post-pr-comment: false\n      upload-sarif: false\n",
            current_release()
        ),
    )
    .expect("write limited workflow");
    let limited = run_github_status(dir.path()).expect("limited status");
    let pr_comment_error = verify_github_status(&limited, false, false, None, true, false)
        .expect_err("disabled PR comment should fail");
    assert!(
        pr_comment_error
            .to_string()
            .contains("PR comment is disabled")
    );
    let sarif_error = verify_github_status(&limited, false, false, None, false, true)
        .expect_err("disabled SARIF should fail");
    assert!(sarif_error.to_string().contains("SARIF upload is disabled"));
}

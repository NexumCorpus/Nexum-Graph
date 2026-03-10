use nex_cli::check_pipeline::install_check_hook;
use nex_cli::github_pipeline::default_workflow_path;
use nex_cli::output::format_start_report;
use nex_cli::start_pipeline::run_start;
use std::fs;
use std::path::Path;

fn init_repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    let repo = git2::Repository::init(dir.path()).expect("init repo");

    write_and_commit(
        &repo,
        "src/lib.rs",
        "fn validate(input: &str) -> bool {\n    !input.is_empty()\n}\n",
        "initial commit",
    );
    write_and_commit(
        &repo,
        "src/lib.rs",
        "fn validate(input: &str) -> bool {\n    input.len() > 1\n}\n\nfn helper() -> usize {\n    1\n}\n",
        "second commit",
    );

    dir
}

fn write_and_commit(repo: &git2::Repository, relative_path: &str, contents: &str, message: &str) {
    let full_path = repo.workdir().unwrap().join(relative_path);
    if let Some(parent) = full_path.parent() {
        fs::create_dir_all(parent).expect("mkdirs");
    }
    fs::write(&full_path, contents).expect("write file");

    let mut index = repo.index().expect("index");
    index
        .add_path(Path::new(relative_path))
        .expect("add path to index");
    index.write().expect("write index");
    let tree_id = index.write_tree().expect("write tree");
    let tree = repo.find_tree(tree_id).expect("find tree");
    let signature = git2::Signature::now("Nexum Graph", "nexum@example.com").expect("signature");

    let parent_commit = repo
        .head()
        .ok()
        .and_then(|head| head.target())
        .and_then(|oid| repo.find_commit(oid).ok());

    match parent_commit {
        Some(parent) => {
            repo.commit(
                Some("HEAD"),
                &signature,
                &signature,
                message,
                &tree,
                &[&parent],
            )
            .expect("commit");
        }
        None => {
            repo.commit(Some("HEAD"), &signature, &signature, message, &tree, &[])
                .expect("initial commit");
        }
    }
}

#[tokio::test(flavor = "current_thread")]
async fn run_start_guides_new_repo_setup() {
    let dir = init_repo();

    let report = run_start(dir.path(), "HEAD~1", "HEAD")
        .await
        .expect("start report");

    assert!(!report.hook_installed);
    assert!(!report.auth_configured);
    assert_eq!(report.next_steps.len(), 4);
    assert_eq!(report.next_steps[1].command, "nex check --install-hook");
    assert_eq!(
        report.next_steps[2].command,
        "nex github init --gate-mode errors-only"
    );
    assert!(
        report.next_steps[3]
            .command
            .contains("nex auth init --agent alice --agent bob"),
    );

    let text = format_start_report(&report, "text");
    assert!(text.contains("Nexum Graph Start"));
    assert!(text.contains("Next steps"));
    assert!(text.contains("nex check --install-hook"));
    assert!(text.contains("nex github init --gate-mode errors-only"));

    let html = format_start_report(&report, "html");
    assert!(html.starts_with("<!DOCTYPE html>"));
    assert!(html.contains("Nexum Graph Start Report"));
    assert!(html.contains("Activation score"));
    assert!(html.contains("nex check --install-hook"));
    assert!(html.contains("nex github init --gate-mode errors-only"));
}

#[tokio::test(flavor = "current_thread")]
async fn run_start_marks_hook_complete_after_install() {
    let dir = init_repo();
    install_check_hook(dir.path(), false).expect("install hook");

    let report = run_start(dir.path(), "HEAD~1", "HEAD")
        .await
        .expect("start report");

    assert!(report.hook_installed);
    assert!(report.hook_healthy);
    assert!(
        report.next_steps[1]
            .reason
            .contains("already has the Nexum Graph merge hook"),
    );

    let json = format_start_report(&report, "json");
    assert!(json.contains("\"hook_installed\": true"));
    assert!(json.contains("\"next_steps\""));
}

#[tokio::test(flavor = "current_thread")]
async fn run_start_flags_outdated_github_workflow() {
    let dir = init_repo();
    let workflow_path = default_workflow_path(dir.path());
    fs::create_dir_all(workflow_path.parent().expect("workflow dir")).expect("mkdirs");
    fs::write(
        &workflow_path,
        "name: Semantic Check\non:\n  pull_request:\n\njobs:\n  semantic-check:\n    uses: NexumCorpus/Nexum-Graph/.github/workflows/reusable-semantic-check.yml@v0.0.9\n    with:\n      format: github\n      gate-mode: advisory\n      post-pr-comment: false\n      upload-sarif: false\n",
    )
    .expect("write outdated workflow");

    let report = run_start(dir.path(), "HEAD~1", "HEAD")
        .await
        .expect("start report");

    assert_eq!(
        report.next_steps[2].status,
        nex_cli::start_pipeline::StartStepStatus::Recommended
    );
    assert_eq!(
        report.next_steps[2].command,
        "nex github init --gate-mode errors-only --force"
    );
    assert!(report.next_steps[2].reason.contains("pinned to v0.0.9"));
}

#[tokio::test(flavor = "current_thread")]
async fn run_start_treats_advisory_github_workflow_as_partial() {
    let dir = init_repo();
    let workflow_path = default_workflow_path(dir.path());
    let current_release = format!("v{}", env!("CARGO_PKG_VERSION"));
    fs::create_dir_all(workflow_path.parent().expect("workflow dir")).expect("mkdirs");
    fs::write(
        &workflow_path,
        format!(
            "name: Semantic Check\non:\n  pull_request:\n\njobs:\n  semantic-check:\n    uses: NexumCorpus/Nexum-Graph/.github/workflows/reusable-semantic-check.yml@{}\n    with:\n      format: github\n      gate-mode: advisory\n      post-pr-comment: true\n      upload-sarif: true\n",
            current_release
        ),
    )
    .expect("write advisory workflow");

    let report = run_start(dir.path(), "HEAD~1", "HEAD")
        .await
        .expect("start report");

    assert_eq!(
        report.next_steps[2].status,
        nex_cli::start_pipeline::StartStepStatus::Ready
    );
    assert_eq!(
        report.next_steps[2].command,
        "nex github init --gate-mode errors-only --force"
    );
    assert!(
        report.next_steps[2]
            .reason
            .contains("visibility-only reporting")
    );
}

#[tokio::test(flavor = "current_thread")]
async fn run_start_treats_limited_review_surfaces_as_partial() {
    let dir = init_repo();
    let workflow_path = default_workflow_path(dir.path());
    let current_release = format!("v{}", env!("CARGO_PKG_VERSION"));
    fs::create_dir_all(workflow_path.parent().expect("workflow dir")).expect("mkdirs");
    fs::write(
        &workflow_path,
        format!(
            "name: Semantic Check\non:\n  pull_request:\n\njobs:\n  semantic-check:\n    uses: NexumCorpus/Nexum-Graph/.github/workflows/reusable-semantic-check.yml@{}\n    with:\n      format: github\n      gate-mode: strict\n      post-pr-comment: false\n      upload-sarif: true\n",
            current_release
        ),
    )
    .expect("write limited workflow");

    let report = run_start(dir.path(), "HEAD~1", "HEAD")
        .await
        .expect("start report");

    assert_eq!(
        report.next_steps[2].status,
        nex_cli::start_pipeline::StartStepStatus::Ready
    );
    assert_eq!(
        report.next_steps[2].command,
        "nex github init --gate-mode strict --force"
    );
    assert!(
        report.next_steps[2]
            .reason
            .contains("standard review surface is incomplete")
    );
}

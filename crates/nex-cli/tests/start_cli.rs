use nex_cli::check_pipeline::install_check_hook;
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
    assert_eq!(report.next_steps.len(), 3);
    assert_eq!(report.next_steps[1].command, "nex check --install-hook");
    assert!(
        report.next_steps[2]
            .command
            .contains("nex auth init --agent alice --agent bob"),
    );

    let text = format_start_report(&report, "text");
    assert!(text.contains("Nexum Graph Start"));
    assert!(text.contains("Next steps"));
    assert!(text.contains("nex check --install-hook"));

    let html = format_start_report(&report, "html");
    assert!(html.starts_with("<!DOCTYPE html>"));
    assert!(html.contains("Nexum Graph Start Report"));
    assert!(html.contains("Activation score"));
    assert!(html.contains("nex check --install-hook"));
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

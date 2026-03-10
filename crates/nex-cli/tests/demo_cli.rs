use nex_cli::demo_pipeline::run_demo;
use nex_cli::output::format_demo_report;
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

fn init_single_commit_repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    let repo = git2::Repository::init(dir.path()).expect("init repo");
    write_and_commit(
        &repo,
        "src/main.py",
        "def validate(value: str) -> bool:\n    return len(value) > 0\n",
        "initial commit",
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
async fn run_demo_reports_repo_summary_and_diff() {
    let dir = init_repo();

    let report = run_demo(dir.path(), "HEAD~1", "HEAD")
        .await
        .expect("demo report");

    assert_eq!(report.base_ref, "HEAD~1");
    assert!(report.head_commit.starts_with("HEAD ("));
    assert!(
        report
            .detected_languages
            .iter()
            .any(|language| language == "Rust")
    );
    assert_eq!(report.indexed_files, 1);
    assert!(report.semantic_units >= 2);
    assert!(report.dependency_edges <= report.semantic_units);
    assert!(report.current_diff.available);
    assert_eq!(report.current_diff.added, 1);
    assert_eq!(report.current_diff.modified, 1);
    assert_eq!(report.active_locks, 0);
    assert_eq!(report.event_count, 0);
    assert!(!report.auth_configured);

    let text = format_demo_report(&report, "text");
    assert!(text.contains("Nexum Graph Demo"));
    assert!(text.contains("Current semantic diff: HEAD~1 -> HEAD"));
    assert!(text.contains("Highlights:"));

    let html = format_demo_report(&report, "html");
    assert!(html.starts_with("<!DOCTYPE html>"));
    assert!(html.contains("Nexum Graph Demo Report"));
    assert!(html.contains("Current semantic change surface"));
    assert!(html.contains("nex check --install-hook"));
}

#[tokio::test(flavor = "current_thread")]
async fn run_demo_degrades_when_base_ref_is_unavailable() {
    let dir = init_single_commit_repo();

    let report = run_demo(dir.path(), "HEAD~1", "HEAD")
        .await
        .expect("demo report");

    assert!(
        report
            .detected_languages
            .iter()
            .any(|language| language == "Python")
    );
    assert!(!report.current_diff.available);
    assert!(report.current_diff.unavailable_reason.is_some());

    let text = format_demo_report(&report, "text");
    assert!(text.contains("Unavailable:"));

    let html = format_demo_report(&report, "html");
    assert!(html.contains("Diff preview needs manual refs"));
}

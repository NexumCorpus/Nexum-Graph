use clap::Parser;
use nex_cli::check_pipeline::{
    CheckHookInstallStatus, install_check_hook, pre_merge_commit_hook_script,
};
use nex_cli::cli::{Cli, Commands};
use std::path::Path;

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
fn install_check_hook_writes_default_pre_merge_hook() {
    let (_dir, repo) = setup_repo();
    let repo_path = repo.workdir().unwrap();

    let result = install_check_hook(repo_path, false).expect("install hook");
    let hook_path = repo.path().join("hooks").join("pre-merge-commit");

    assert_eq!(result.hook_path, hook_path);
    assert_eq!(result.status, CheckHookInstallStatus::Installed);
    assert!(!result.uses_custom_hooks_path);
    assert_eq!(
        std::fs::read_to_string(&hook_path).expect("read hook"),
        pre_merge_commit_hook_script()
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
    let hook_path = repo_path.join(".githooks").join("pre-merge-commit");

    assert_eq!(result.hook_path, hook_path);
    assert!(result.uses_custom_hooks_path);
    assert_eq!(result.status, CheckHookInstallStatus::Installed);
}

#[test]
fn install_check_hook_requires_force_to_replace_existing_script() {
    let (_dir, repo) = setup_repo();
    let repo_path = repo.workdir().unwrap();
    let hook_path = repo.path().join("hooks").join("pre-merge-commit");
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
        pre_merge_commit_hook_script()
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

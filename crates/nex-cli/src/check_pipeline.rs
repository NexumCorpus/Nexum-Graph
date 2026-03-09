use git2::Repository;
use nex_core::{CodexError, CodexResult, ConflictReport, atomic_write_bytes};
use serde::Serialize;
use std::path::{Path, PathBuf};

const CHECK_HOOK_NAME: &str = "pre-merge-commit";

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CheckHookInstallStatus {
    Installed,
    Updated,
    Unchanged,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct CheckHookInstallResult {
    pub hook_name: &'static str,
    pub hook_path: PathBuf,
    pub status: CheckHookInstallStatus,
    pub uses_custom_hooks_path: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct CheckHookStatus {
    pub hook_name: &'static str,
    pub hook_path: PathBuf,
    pub installed: bool,
    pub matches_expected: bool,
    pub uses_custom_hooks_path: bool,
}

pub fn run_check(repo_path: &Path, branch_a: &str, branch_b: &str) -> CodexResult<ConflictReport> {
    nex_coord::ConflictDetector::detect(repo_path, branch_a, branch_b)
}

pub fn install_check_hook(repo_path: &Path, force: bool) -> CodexResult<CheckHookInstallResult> {
    let repo = Repository::discover(repo_path).map_err(|err| CodexError::Git(err.to_string()))?;
    let workdir = repo.workdir().ok_or_else(|| {
        CodexError::Git("cannot install nex check hook in a bare repository".into())
    })?;
    let (hooks_dir, uses_custom_hooks_path) = resolve_hooks_dir(&repo, workdir)?;
    let hook_path = hooks_dir.join(CHECK_HOOK_NAME);
    let script = pre_merge_commit_hook_script();
    let script_bytes = script.as_bytes();

    let status = match std::fs::read(&hook_path) {
        Ok(existing) if existing == script_bytes => CheckHookInstallStatus::Unchanged,
        Ok(_) if !force => {
            return Err(CodexError::Coordination(format!(
                "hook already exists at {} (pass --force to replace it)",
                hook_path.display()
            )));
        }
        Ok(_) => CheckHookInstallStatus::Updated,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => CheckHookInstallStatus::Installed,
        Err(err) => return Err(err.into()),
    };

    if status != CheckHookInstallStatus::Unchanged {
        atomic_write_bytes(&hook_path, script_bytes)?;
        set_hook_executable(&hook_path)?;
    }

    Ok(CheckHookInstallResult {
        hook_name: CHECK_HOOK_NAME,
        hook_path,
        status,
        uses_custom_hooks_path,
    })
}

pub fn check_hook_status(repo_path: &Path) -> CodexResult<CheckHookStatus> {
    let repo = Repository::discover(repo_path).map_err(|err| CodexError::Git(err.to_string()))?;
    let workdir = repo.workdir().ok_or_else(|| {
        CodexError::Git("cannot inspect nex check hook in a bare repository".into())
    })?;
    let (hooks_dir, uses_custom_hooks_path) = resolve_hooks_dir(&repo, workdir)?;
    let hook_path = hooks_dir.join(CHECK_HOOK_NAME);
    let expected = pre_merge_commit_hook_script().into_bytes();

    let (installed, matches_expected) = match std::fs::read(&hook_path) {
        Ok(existing) => (true, existing == expected),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => (false, false),
        Err(err) => return Err(err.into()),
    };

    Ok(CheckHookStatus {
        hook_name: CHECK_HOOK_NAME,
        hook_path,
        installed,
        matches_expected,
        uses_custom_hooks_path,
    })
}

pub fn pre_merge_commit_hook_script() -> String {
    r#"#!/usr/bin/env sh
set -eu

if command -v nex >/dev/null 2>&1; then
    nex_bin=nex
elif command -v nex.exe >/dev/null 2>&1; then
    nex_bin=nex.exe
else
    printf 'error: nex is not on PATH; install Nexum Graph before running this hook.\n' >&2
    exit 1
fi

repo_root=$(git rev-parse --show-toplevel 2>/dev/null || pwd)
merge_head=$(git rev-parse --verify MERGE_HEAD 2>/dev/null || true)

if [ -z "$merge_head" ]; then
    exit 0
fi

printf 'nex check hook: checking HEAD against %s\n' "$merge_head" >&2
exec "$nex_bin" check HEAD "$merge_head" --repo-path "$repo_root" --format text
"#
    .to_string()
}

fn resolve_hooks_dir(repo: &Repository, workdir: &Path) -> CodexResult<(PathBuf, bool)> {
    let config = repo
        .config()
        .map_err(|err| CodexError::Git(err.to_string()))?;
    match config.get_path("core.hooksPath") {
        Ok(hooks_path) => {
            let resolved = if hooks_path.is_absolute() {
                hooks_path
            } else {
                workdir.join(hooks_path)
            };
            Ok((resolved, true))
        }
        Err(err) if err.class() == git2::ErrorClass::Config => {
            Ok((repo.path().join("hooks"), false))
        }
        Err(err) => Err(CodexError::Git(err.to_string())),
    }
}

#[cfg(unix)]
fn set_hook_executable(path: &Path) -> CodexResult<()> {
    use std::os::unix::fs::PermissionsExt;

    let mut permissions = std::fs::metadata(path)?.permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(path, permissions)?;
    Ok(())
}

#[cfg(not(unix))]
fn set_hook_executable(_path: &Path) -> CodexResult<()> {
    Ok(())
}

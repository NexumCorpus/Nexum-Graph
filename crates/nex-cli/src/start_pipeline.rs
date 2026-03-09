use crate::check_pipeline;
use crate::demo_pipeline::{self, DemoReport};
use nex_core::CodexResult;
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize)]
pub struct StartReport {
    pub repo_path: PathBuf,
    pub demo: DemoReport,
    pub hook_installed: bool,
    pub hook_healthy: bool,
    pub hook_path: PathBuf,
    pub auth_configured: bool,
    pub next_steps: Vec<StartStep>,
}

#[derive(Debug, Clone, Serialize)]
pub struct StartStep {
    pub title: String,
    pub status: StartStepStatus,
    pub command: String,
    pub reason: String,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StartStepStatus {
    Recommended,
    Ready,
    Complete,
}

pub async fn run_start(
    repo_path: &Path,
    base_ref: &str,
    head_ref: &str,
) -> CodexResult<StartReport> {
    let demo = demo_pipeline::run_demo(repo_path, base_ref, head_ref).await?;
    let hook = check_pipeline::check_hook_status(repo_path)?;

    let next_steps = vec![
        StartStep {
            title: "Inspect the current semantic diff".to_string(),
            status: if demo.current_diff.available {
                StartStepStatus::Ready
            } else {
                StartStepStatus::Recommended
            },
            command: format!("nex diff {} {}", demo.base_ref, demo.head_ref),
            reason: if demo.current_diff.available {
                "See the exact semantic changes behind the current repo snapshot.".to_string()
            } else {
                "Your repo snapshot worked, but the default diff preview was unavailable. Run an explicit diff on refs you care about.".to_string()
            },
        },
        StartStep {
            title: "Install the semantic merge guard".to_string(),
            status: if hook.installed && hook.matches_expected {
                StartStepStatus::Complete
            } else {
                StartStepStatus::Recommended
            },
            command: "nex check --install-hook".to_string(),
            reason: if hook.installed && hook.matches_expected {
                format!(
                    "The repo already has the Nexum Graph merge hook at {}.",
                    hook.hook_path.display()
                )
            } else if hook.installed {
                format!(
                    "A hook exists at {}, but it is not the current Nexum Graph script. Reinstall to get the semantic merge check.",
                    hook.hook_path.display()
                )
            } else {
                "Install a local pre-merge hook so merge commits run `nex check` before they land."
                    .to_string()
            },
        },
        StartStep {
            title: "Turn on multi-agent coordination".to_string(),
            status: if demo.auth_configured {
                StartStepStatus::Ready
            } else {
                StartStepStatus::Recommended
            },
            command: if demo.auth_configured {
                "nex serve --host 127.0.0.1 --port 4000".to_string()
            } else {
                "nex auth init --agent alice --agent bob && nex serve --host 127.0.0.1 --port 4000"
                    .to_string()
            },
            reason: if demo.auth_configured {
                "Auth is already configured, so you can start the local coordination server immediately."
                    .to_string()
            } else {
                "Bootstrap auth once, then start the coordination server for multi-agent work."
                    .to_string()
            },
        },
    ];

    Ok(StartReport {
        repo_path: demo.repo_path.clone(),
        hook_installed: hook.installed,
        hook_healthy: hook.matches_expected,
        hook_path: hook.hook_path,
        auth_configured: demo.auth_configured,
        demo,
        next_steps,
    })
}

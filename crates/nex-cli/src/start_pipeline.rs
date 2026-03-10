use crate::check_pipeline;
use crate::demo_pipeline::{self, DemoReport};
use crate::github_pipeline;
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
    let github_status = github_pipeline::run_github_status(repo_path)?;
    let github_assessment = github_pipeline::assess_github_status(&github_status);
    let github_init_command = github_assessment
        .recommended_command
        .clone()
        .unwrap_or_else(|| "nex github status --require-current --min-gate-mode errors-only --require-pr-comment --require-sarif".to_string());

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
            title: "Install the GitHub pull request gate".to_string(),
            status: match github_assessment.rollout_stage {
                github_pipeline::GitHubWorkflowRolloutStage::Ready => StartStepStatus::Complete,
                github_pipeline::GitHubWorkflowRolloutStage::Advisory
                | github_pipeline::GitHubWorkflowRolloutStage::LimitedReview => {
                    StartStepStatus::Ready
                }
                _ => StartStepStatus::Recommended,
            },
            command: github_init_command,
            reason: if github_assessment.rollout_stage
                == github_pipeline::GitHubWorkflowRolloutStage::Ready
            {
                format!(
                    "The repo already has the current Nexum Graph pull request workflow at {} with an enforcing gate mode and the full review surface enabled.",
                    github_status.workflow_path.display(),
                )
            } else if github_assessment.rollout_stage
                == github_pipeline::GitHubWorkflowRolloutStage::LimitedReview
            {
                format!(
                    "The repo has the current Nexum Graph pull request workflow at {}, but the standard review surface is incomplete (PR comment: {}, SARIF: {}). Reinstall it to restore the sticky PR summary and code-scanning output.",
                    github_status.workflow_path.display(),
                    if github_status.post_pr_comment == Some(true) {
                        "enabled"
                    } else {
                        "disabled"
                    },
                    if github_status.upload_sarif == Some(true) {
                        "enabled"
                    } else {
                        "disabled"
                    },
                )
            } else if github_assessment.rollout_stage
                == github_pipeline::GitHubWorkflowRolloutStage::Advisory
            {
                format!(
                    "The repo has the current Nexum Graph pull request workflow at {}, but it is running in `{}` mode. Upgrade it to `errors-only` or `strict` when you want a real merge gate instead of visibility-only reporting.",
                    github_status.workflow_path.display(),
                    github_status.gate_mode.as_deref().unwrap_or("unknown"),
                )
            } else if github_assessment.rollout_stage
                == github_pipeline::GitHubWorkflowRolloutStage::Outdated
            {
                format!(
                    "The repo has a Nexum Graph pull request workflow at {}, but it is pinned to {} instead of {}. Reinstall it to pick up the current PR comment, SARIF, and insights behavior.",
                    github_status.workflow_path.display(),
                    github_status
                        .workflow_ref
                        .as_deref()
                        .unwrap_or("an unknown ref"),
                    github_status.current_ref,
                )
            } else if github_assessment.rollout_stage
                == github_pipeline::GitHubWorkflowRolloutStage::Custom
            {
                format!(
                    "A workflow exists at {}, but it is not the managed Nexum Graph reusable workflow. Reinstall it if you want the standard PR comment, SARIF, and insights artifacts.",
                    github_status.workflow_path.display()
                )
            } else {
                "Write the reusable GitHub workflow so pull requests get semantic comments, SARIF, and merge-risk artifacts."
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

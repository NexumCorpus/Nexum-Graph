use nex_core::{CodexError, CodexResult};
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};

const DEFAULT_WORKFLOW_FILE: &str = ".github/workflows/nexum-graph-semantic-check.yml";
const REUSABLE_WORKFLOW_PREFIX: &str =
    "NexumCorpus/Nexum-Graph/.github/workflows/reusable-semantic-check.yml@";

#[derive(Debug, Clone, Serialize)]
pub struct GitHubWorkflowInitResult {
    pub repo_path: PathBuf,
    pub workflow_path: PathBuf,
    pub workflow_name: String,
    pub gate_mode: String,
    pub post_pr_comment: bool,
    pub upload_sarif: bool,
    pub replaced_existing: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct GitHubWorkflowStatus {
    pub repo_path: PathBuf,
    pub workflow_path: PathBuf,
    pub exists: bool,
    pub managed_by_nexum_graph: bool,
    pub workflow_ref: Option<String>,
    pub current_ref: String,
    pub up_to_date: Option<bool>,
    pub workflow_name: Option<String>,
    pub gate_mode: Option<String>,
    pub post_pr_comment: Option<bool>,
    pub upload_sarif: Option<bool>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GitHubWorkflowRolloutStage {
    Missing,
    Custom,
    Outdated,
    Advisory,
    LimitedReview,
    Ready,
}

#[derive(Debug, Clone, Serialize)]
pub struct GitHubWorkflowAssessment {
    pub rollout_stage: GitHubWorkflowRolloutStage,
    pub gate_enforcing: bool,
    pub review_surfaces_enabled: bool,
    pub branch_protection_ready: bool,
    pub recommended_command: Option<String>,
}

pub fn default_workflow_path(repo_path: &Path) -> PathBuf {
    repo_path.join(DEFAULT_WORKFLOW_FILE)
}

pub fn run_github_status(repo_path: &Path) -> CodexResult<GitHubWorkflowStatus> {
    let workflow_path = default_workflow_path(repo_path);
    let current_ref = format!("v{}", env!("CARGO_PKG_VERSION"));
    if !workflow_path.exists() {
        return Ok(GitHubWorkflowStatus {
            repo_path: repo_path.to_path_buf(),
            workflow_path,
            exists: false,
            managed_by_nexum_graph: false,
            workflow_ref: None,
            current_ref,
            up_to_date: None,
            workflow_name: None,
            gate_mode: None,
            post_pr_comment: None,
            upload_sarif: None,
        });
    }

    let content = fs::read_to_string(&workflow_path)?;
    let workflow_ref = parse_workflow_ref(&content);
    let managed_by_nexum_graph = workflow_ref.is_some();
    let up_to_date = workflow_ref
        .as_ref()
        .map(|workflow_ref| workflow_ref == &current_ref);

    Ok(GitHubWorkflowStatus {
        repo_path: repo_path.to_path_buf(),
        workflow_path,
        exists: true,
        managed_by_nexum_graph,
        workflow_ref,
        current_ref,
        up_to_date,
        workflow_name: parse_scalar(&content, "name"),
        gate_mode: parse_scalar(&content, "gate-mode"),
        post_pr_comment: parse_bool(&content, "post-pr-comment"),
        upload_sarif: parse_bool(&content, "upload-sarif"),
    })
}

pub fn assess_github_status(status: &GitHubWorkflowStatus) -> GitHubWorkflowAssessment {
    let gate_mode = status.gate_mode.as_deref();
    let gate_enforcing = gate_mode_satisfies(gate_mode, "errors-only");
    let review_surfaces_enabled = review_surfaces_enabled(status);
    let rollout_stage = if !status.exists {
        GitHubWorkflowRolloutStage::Missing
    } else if !status.managed_by_nexum_graph {
        GitHubWorkflowRolloutStage::Custom
    } else if status.up_to_date != Some(true) {
        GitHubWorkflowRolloutStage::Outdated
    } else if !gate_enforcing {
        GitHubWorkflowRolloutStage::Advisory
    } else if !review_surfaces_enabled {
        GitHubWorkflowRolloutStage::LimitedReview
    } else {
        GitHubWorkflowRolloutStage::Ready
    };

    let recommended_command = match rollout_stage {
        GitHubWorkflowRolloutStage::Missing => {
            Some("nex github init --gate-mode errors-only".to_string())
        }
        GitHubWorkflowRolloutStage::Custom => {
            Some("nex github init --gate-mode errors-only --force".to_string())
        }
        GitHubWorkflowRolloutStage::Outdated => Some(format!(
            "nex github init --gate-mode {} --force",
            if gate_enforcing {
                gate_mode.unwrap_or("errors-only")
            } else {
                "errors-only"
            }
        )),
        GitHubWorkflowRolloutStage::Advisory => {
            Some("nex github init --gate-mode errors-only --force".to_string())
        }
        GitHubWorkflowRolloutStage::LimitedReview => Some(format!(
            "nex github init --gate-mode {} --force",
            gate_mode.unwrap_or("errors-only")
        )),
        GitHubWorkflowRolloutStage::Ready => None,
    };

    GitHubWorkflowAssessment {
        rollout_stage,
        gate_enforcing,
        review_surfaces_enabled,
        branch_protection_ready: rollout_stage == GitHubWorkflowRolloutStage::Ready,
        recommended_command,
    }
}

pub fn verify_github_status(
    status: &GitHubWorkflowStatus,
    require_managed: bool,
    require_current: bool,
    min_gate_mode: Option<&str>,
    require_pr_comment: bool,
    require_sarif: bool,
) -> CodexResult<()> {
    if require_current {
        if !status.exists {
            return Err(CodexError::Coordination(
                "GitHub workflow is not installed; run `nex github init --gate-mode errors-only`"
                    .to_string(),
            ));
        }
        if !status.managed_by_nexum_graph {
            return Err(CodexError::Coordination(
                "GitHub workflow is custom; reinstall the managed Nexum Graph workflow with `nex github init --gate-mode errors-only --force`".to_string(),
            ));
        }
        if status.up_to_date != Some(true) {
            let gate_mode = status.gate_mode.as_deref().unwrap_or("errors-only");
            let pinned = status.workflow_ref.as_deref().unwrap_or("an unknown ref");
            return Err(CodexError::Coordination(format!(
                "GitHub workflow is pinned to {pinned} instead of {}; run `nex github init --gate-mode {gate_mode} --force`",
                status.current_ref,
            )));
        }
    } else if require_managed {
        if !status.exists {
            return Err(CodexError::Coordination(
                "GitHub workflow is not installed; run `nex github init --gate-mode errors-only`"
                    .to_string(),
            ));
        }
        if !status.managed_by_nexum_graph {
            return Err(CodexError::Coordination(
                "GitHub workflow is custom; reinstall the managed Nexum Graph workflow with `nex github init --gate-mode errors-only --force`".to_string(),
            ));
        }
    }

    if let Some(min_gate_mode) = min_gate_mode {
        if gate_mode_rank(min_gate_mode).is_none() {
            return Err(CodexError::Coordination(format!(
                "unsupported minimum gate mode `{min_gate_mode}`; expected advisory, errors-only, or strict"
            )));
        }
        if !status.exists {
            return Err(CodexError::Coordination(
                "GitHub workflow is not installed; run `nex github init --gate-mode errors-only`"
                    .to_string(),
            ));
        }

        let actual_gate_mode = status
            .gate_mode
            .as_deref()
            .ok_or_else(|| {
                CodexError::Coordination(
                    "GitHub workflow gate mode could not be determined; reinstall the managed Nexum Graph workflow with `nex github init --gate-mode errors-only --force`".to_string(),
                )
            })?;
        if !gate_mode_satisfies(Some(actual_gate_mode), min_gate_mode) {
            return Err(CodexError::Coordination(format!(
                "GitHub workflow gate mode is `{actual_gate_mode}`, below required `{min_gate_mode}`; run `nex github init --gate-mode {min_gate_mode} --force`"
            )));
        }
    }

    if require_pr_comment || require_sarif {
        if !status.exists {
            return Err(CodexError::Coordination(
                "GitHub workflow is not installed; run `nex github init --gate-mode errors-only`"
                    .to_string(),
            ));
        }
        if !status.managed_by_nexum_graph {
            return Err(CodexError::Coordination(
                "GitHub workflow is custom; reinstall the managed Nexum Graph workflow with `nex github init --gate-mode errors-only --force`".to_string(),
            ));
        }

        let gate_mode = status.gate_mode.as_deref().unwrap_or("errors-only");
        if require_pr_comment && status.post_pr_comment != Some(true) {
            return Err(CodexError::Coordination(format!(
                "GitHub workflow PR comment is disabled; run `nex github init --gate-mode {gate_mode} --force`"
            )));
        }
        if require_sarif && status.upload_sarif != Some(true) {
            return Err(CodexError::Coordination(format!(
                "GitHub workflow SARIF upload is disabled; run `nex github init --gate-mode {gate_mode} --force`"
            )));
        }
    }

    Ok(())
}

pub fn run_github_init(
    repo_path: &Path,
    workflow_name: &str,
    gate_mode: &str,
    post_pr_comment: bool,
    upload_sarif: bool,
    force: bool,
) -> CodexResult<GitHubWorkflowInitResult> {
    if !matches!(gate_mode, "strict" | "errors-only" | "advisory") {
        return Err(CodexError::Coordination(format!(
            "unsupported gate mode `{gate_mode}`; expected strict, errors-only, or advisory"
        )));
    }

    let workflow_path = default_workflow_path(repo_path);
    let replaced_existing = workflow_path.exists();
    if replaced_existing && !force {
        return Err(CodexError::Io(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            format!(
                "{} already exists; pass --force to replace it",
                workflow_path.display()
            ),
        )));
    }

    if let Some(parent) = workflow_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(
        &workflow_path,
        render_workflow(workflow_name, gate_mode, post_pr_comment, upload_sarif),
    )?;

    Ok(GitHubWorkflowInitResult {
        repo_path: repo_path.to_path_buf(),
        workflow_path,
        workflow_name: workflow_name.to_string(),
        gate_mode: gate_mode.to_string(),
        post_pr_comment,
        upload_sarif,
        replaced_existing,
    })
}

fn render_workflow(
    workflow_name: &str,
    gate_mode: &str,
    post_pr_comment: bool,
    upload_sarif: bool,
) -> String {
    format!(
        "name: {workflow_name}\n\non:\n  pull_request:\n\njobs:\n  semantic-check:\n    uses: NexumCorpus/Nexum-Graph/.github/workflows/reusable-semantic-check.yml@v{version}\n    with:\n      format: github\n      gate-mode: {gate_mode}\n      post-pr-comment: {post_pr_comment}\n      upload-sarif: {upload_sarif}\n",
        workflow_name = workflow_name,
        version = env!("CARGO_PKG_VERSION"),
        gate_mode = gate_mode,
        post_pr_comment = if post_pr_comment { "true" } else { "false" },
        upload_sarif = if upload_sarif { "true" } else { "false" },
    )
}

fn parse_scalar(content: &str, key: &str) -> Option<String> {
    content.lines().find_map(|line| {
        let trimmed = line.trim();
        let prefix = format!("{key}:");
        trimmed
            .strip_prefix(&prefix)
            .map(|value| value.trim().trim_matches('"').to_string())
    })
}

fn parse_bool(content: &str, key: &str) -> Option<bool> {
    parse_scalar(content, key).and_then(|value| match value.as_str() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    })
}

fn parse_workflow_ref(content: &str) -> Option<String> {
    parse_scalar(content, "uses").and_then(|value| {
        value
            .strip_prefix(REUSABLE_WORKFLOW_PREFIX)
            .map(ToString::to_string)
    })
}

pub fn gate_mode_satisfies(actual: Option<&str>, minimum: &str) -> bool {
    let Some(min_rank) = gate_mode_rank(minimum) else {
        return false;
    };
    let Some(actual_rank) = actual.and_then(gate_mode_rank) else {
        return false;
    };
    actual_rank >= min_rank
}

pub fn review_surfaces_enabled(status: &GitHubWorkflowStatus) -> bool {
    status.post_pr_comment == Some(true) && status.upload_sarif == Some(true)
}

fn gate_mode_rank(mode: &str) -> Option<u8> {
    match mode {
        "advisory" => Some(0),
        "errors-only" => Some(1),
        "strict" => Some(2),
        _ => None,
    }
}

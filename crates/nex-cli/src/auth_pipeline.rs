//! Auth config bootstrap and lifecycle helpers for `nex auth` and `nex serve`.

use nex_core::{CodexError, CodexResult, atomic_write_json, backup_path, load_json_with_backup};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use uuid::Uuid;

pub const ENV_SERVER_AUTH_CONFIG: &str = "NEX_SERVER_AUTH_CONFIG";
pub const SERVER_AUTH_CONFIG_FILE: &str = "server-auth.json";
pub(crate) const AUTH_HASH_PREFIX: &str = "blake3:";
const CURRENT_AUTH_CONFIG_VERSION: u32 = 2;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AuthConfigFile {
    #[serde(default = "current_auth_config_version")]
    pub format_version: u32,
    #[serde(default)]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub shared_tokens: Vec<String>,
    #[serde(default)]
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub agent_tokens: HashMap<String, Vec<String>>,
    #[serde(default)]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub revoked_tokens: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shared_token_hashes: Vec<String>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub agent_token_hashes: HashMap<String, Vec<String>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub revoked_token_hashes: Vec<String>,
}

impl Default for AuthConfigFile {
    fn default() -> Self {
        Self {
            format_version: current_auth_config_version(),
            shared_tokens: Vec::new(),
            agent_tokens: HashMap::new(),
            revoked_tokens: Vec::new(),
            shared_token_hashes: Vec::new(),
            agent_token_hashes: HashMap::new(),
            revoked_token_hashes: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AuthConfigMode {
    Disabled,
    Shared,
    Agent,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct AuthAgentStatus {
    pub agent_name: String,
    pub active_tokens: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct AuthStatus {
    pub path: PathBuf,
    pub exists: bool,
    pub using_backup: bool,
    pub mode: AuthConfigMode,
    pub shared_token_count: usize,
    pub revoked_token_count: usize,
    pub agents: Vec<AuthAgentStatus>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct IssuedAuthToken {
    pub agent_name: Option<String>,
    pub token: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct AuthInitResult {
    pub path: PathBuf,
    pub mode: AuthConfigMode,
    pub replaced_existing: bool,
    pub issued: Vec<IssuedAuthToken>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct AuthIssueResult {
    pub path: PathBuf,
    pub mode: AuthConfigMode,
    pub issued: IssuedAuthToken,
    pub active_token_count: usize,
    pub revoked_token_count: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct AuthRevokeResult {
    pub path: PathBuf,
    pub mode: AuthConfigMode,
    pub token: String,
    pub affected_agent: Option<String>,
    pub removed: bool,
    pub already_revoked: bool,
    pub active_token_count: usize,
    pub revoked_token_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthIssueTarget {
    Shared,
    Agent(String),
}

impl AuthConfigFile {
    pub fn normalize(mut self) -> CodexResult<Self> {
        self.format_version = current_auth_config_version();
        let legacy_shared_tokens = std::mem::take(&mut self.shared_tokens);
        let legacy_agent_tokens = std::mem::take(&mut self.agent_tokens);
        let legacy_revoked_tokens = std::mem::take(&mut self.revoked_tokens);
        let existing_shared_hashes = std::mem::take(&mut self.shared_token_hashes);
        let existing_agent_hashes = std::mem::take(&mut self.agent_token_hashes);
        let existing_revoked_hashes = std::mem::take(&mut self.revoked_token_hashes);

        self.shared_token_hashes = normalize_token_entries(
            existing_shared_hashes
                .into_iter()
                .chain(legacy_shared_tokens),
        )?;
        self.revoked_token_hashes = normalize_token_entries(
            existing_revoked_hashes
                .into_iter()
                .chain(legacy_revoked_tokens),
        )?;
        self.agent_token_hashes =
            normalize_agent_token_map(existing_agent_hashes, legacy_agent_tokens)?;

        if !self.shared_token_hashes.is_empty() && !self.agent_token_hashes.is_empty() {
            return Err(CodexError::Coordination(
                "auth config must use either shared token mode or per-agent token mode, not both"
                    .to_string(),
            ));
        }

        let mut seen_active = HashMap::new();
        for token in &self.shared_token_hashes {
            ensure_unique_active_token(&mut seen_active, None, token)?;
        }
        for (agent_name, tokens) in &self.agent_token_hashes {
            for token in tokens {
                ensure_unique_active_token(&mut seen_active, Some(agent_name), token)?;
            }
        }

        Ok(self)
    }

    pub fn mode(&self) -> AuthConfigMode {
        if !self.agent_token_hashes.is_empty() || !self.agent_tokens.is_empty() {
            AuthConfigMode::Agent
        } else if !self.shared_token_hashes.is_empty() || !self.shared_tokens.is_empty() {
            AuthConfigMode::Shared
        } else {
            AuthConfigMode::Disabled
        }
    }

    pub fn active_token_count(&self) -> usize {
        self.shared_token_hashes.len()
            + self
                .agent_token_hashes
                .values()
                .map(std::vec::Vec::len)
                .sum::<usize>()
    }
}

pub fn default_auth_config_path(repo_path: &Path) -> PathBuf {
    repo_path.join(".nex").join(SERVER_AUTH_CONFIG_FILE)
}

pub fn env_auth_config_path() -> Option<PathBuf> {
    std::env::var(ENV_SERVER_AUTH_CONFIG)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

pub fn discover_repo_auth_config_path(repo_path: &Path) -> Option<PathBuf> {
    let path = default_auth_config_path(repo_path);
    if config_path_exists(&path) {
        Some(path)
    } else {
        None
    }
}

pub fn selected_auth_config_path(repo_path: &Path, auth_config: Option<PathBuf>) -> PathBuf {
    auth_config.unwrap_or_else(|| default_auth_config_path(repo_path))
}

pub fn load_auth_config(path: &Path) -> CodexResult<Option<AuthConfigFile>> {
    load_json_with_backup(path)?
        .map(AuthConfigFile::normalize)
        .transpose()
}

pub fn save_auth_config(path: &Path, config: &AuthConfigFile) -> CodexResult<()> {
    let normalized = config.clone().normalize()?;
    atomic_write_json(path, &normalized)
}

pub fn auth_status(repo_path: &Path, auth_config: Option<PathBuf>) -> CodexResult<AuthStatus> {
    let path = selected_auth_config_path(repo_path, auth_config);
    let exists = config_path_exists(&path);
    let using_backup = !path.exists() && backup_path(&path).exists();
    let Some(config) = load_auth_config(&path)? else {
        return Ok(AuthStatus {
            path,
            exists,
            using_backup,
            mode: AuthConfigMode::Disabled,
            shared_token_count: 0,
            revoked_token_count: 0,
            agents: Vec::new(),
        });
    };

    let mut agents = config
        .agent_token_hashes
        .iter()
        .map(|(agent_name, tokens)| AuthAgentStatus {
            agent_name: agent_name.clone(),
            active_tokens: tokens.len(),
        })
        .collect::<Vec<_>>();
    agents.sort_by(|left, right| left.agent_name.cmp(&right.agent_name));

    Ok(AuthStatus {
        path,
        exists,
        using_backup,
        mode: config.mode(),
        shared_token_count: config.shared_token_hashes.len(),
        revoked_token_count: config.revoked_token_hashes.len(),
        agents,
    })
}

pub fn init_auth_config(
    repo_path: &Path,
    auth_config: Option<PathBuf>,
    agents: &[String],
    shared: bool,
    force: bool,
) -> CodexResult<AuthInitResult> {
    validate_init_request(agents, shared)?;

    let path = selected_auth_config_path(repo_path, auth_config);
    let replaced_existing = config_path_exists(&path);
    if replaced_existing && !force {
        return Err(CodexError::Coordination(format!(
            "auth config already exists at {}; pass --force to replace it",
            path.display()
        )));
    }

    let mut config = AuthConfigFile::default();
    let mut issued = Vec::new();
    if shared {
        let token = generate_token("shared");
        config.shared_token_hashes.push(hash_token(&token));
        issued.push(IssuedAuthToken {
            agent_name: None,
            token,
        });
    } else {
        let agents = normalize_unique_agents(agents)?;
        for agent_name in agents {
            let token = generate_token(&agent_name);
            config
                .agent_token_hashes
                .entry(agent_name.clone())
                .or_default()
                .push(hash_token(&token));
            issued.push(IssuedAuthToken {
                agent_name: Some(agent_name),
                token,
            });
        }
    }

    save_auth_config(&path, &config)?;

    Ok(AuthInitResult {
        path,
        mode: config.mode(),
        replaced_existing,
        issued,
    })
}

pub fn issue_auth_token(
    repo_path: &Path,
    auth_config: Option<PathBuf>,
    target: AuthIssueTarget,
) -> CodexResult<AuthIssueResult> {
    let path = selected_auth_config_path(repo_path, auth_config);
    let mut config = load_auth_config(&path)?.unwrap_or_default();

    let issued = match target {
        AuthIssueTarget::Shared => {
            if !config.agent_token_hashes.is_empty() {
                return Err(CodexError::Coordination(
                    "auth config is in per-agent mode; use `nex auth issue <agent>` instead"
                        .to_string(),
                ));
            }
            let token = generate_token("shared");
            config.shared_token_hashes.push(hash_token(&token));
            IssuedAuthToken {
                agent_name: None,
                token,
            }
        }
        AuthIssueTarget::Agent(agent_name) => {
            if !config.shared_token_hashes.is_empty() {
                return Err(CodexError::Coordination(
                    "auth config is in shared-token mode; use `nex auth issue --shared` instead"
                        .to_string(),
                ));
            }
            let agent_name = normalize_agent_name(&agent_name)?;
            let token = generate_token(&agent_name);
            config
                .agent_token_hashes
                .entry(agent_name.clone())
                .or_default()
                .push(hash_token(&token));
            IssuedAuthToken {
                agent_name: Some(agent_name),
                token,
            }
        }
    };

    save_auth_config(&path, &config)?;

    Ok(AuthIssueResult {
        path,
        mode: config.mode(),
        active_token_count: config.active_token_count(),
        revoked_token_count: config.revoked_token_hashes.len(),
        issued,
    })
}

pub fn revoke_auth_token(
    repo_path: &Path,
    auth_config: Option<PathBuf>,
    token: &str,
) -> CodexResult<AuthRevokeResult> {
    let token = token.trim();
    let token_hash = canonicalize_token_secret(token)?;

    let path = selected_auth_config_path(repo_path, auth_config);
    let mut config = load_auth_config(&path)?.ok_or_else(|| {
        CodexError::Coordination(format!("auth config not found at {}", path.display()))
    })?;

    let already_revoked = config
        .revoked_token_hashes
        .iter()
        .any(|entry| entry == &token_hash);
    let mut removed = remove_token_hash(&mut config.shared_token_hashes, &token_hash);
    let mut affected_agent = None;

    for (agent_name, tokens) in &mut config.agent_token_hashes {
        if remove_token_hash(tokens, &token_hash) {
            removed = true;
            affected_agent = Some(agent_name.clone());
            break;
        }
    }
    config
        .agent_token_hashes
        .retain(|_, tokens| !tokens.is_empty());

    if !removed && !already_revoked {
        return Err(CodexError::Coordination(format!(
            "token not found in auth config: {token}"
        )));
    }

    if !already_revoked {
        config.revoked_token_hashes.push(token_hash);
    }

    save_auth_config(&path, &config)?;

    Ok(AuthRevokeResult {
        path,
        mode: config.mode(),
        token: token.to_string(),
        affected_agent,
        removed,
        already_revoked,
        active_token_count: config.active_token_count(),
        revoked_token_count: config.revoked_token_hashes.len(),
    })
}

fn validate_init_request(agents: &[String], shared: bool) -> CodexResult<()> {
    if shared && !agents.is_empty() {
        return Err(CodexError::Coordination(
            "use either --shared or one or more --agent values, not both".to_string(),
        ));
    }
    if !shared && agents.is_empty() {
        return Err(CodexError::Coordination(
            "auth init requires --shared or at least one --agent".to_string(),
        ));
    }
    Ok(())
}

fn normalize_unique_agents(agents: &[String]) -> CodexResult<Vec<String>> {
    let mut seen = HashSet::new();
    let mut normalized = Vec::new();
    for agent in agents {
        let agent = normalize_agent_name(agent)?;
        if !seen.insert(agent.clone()) {
            return Err(CodexError::Coordination(format!(
                "duplicate agent specified for auth config: {agent}"
            )));
        }
        normalized.push(agent);
    }
    Ok(normalized)
}

fn normalize_agent_name(agent: &str) -> CodexResult<String> {
    let agent = agent.trim();
    if agent.is_empty() {
        return Err(CodexError::Coordination(
            "agent name must not be empty".to_string(),
        ));
    }
    Ok(agent.to_string())
}

fn normalize_token_entries<I>(entries: I) -> CodexResult<Vec<String>>
where
    I: IntoIterator<Item = String>,
{
    let mut seen = HashSet::new();
    let mut normalized = Vec::new();
    for token in entries {
        let token = token.trim();
        if token.is_empty() {
            continue;
        }
        let token_hash = canonicalize_token_secret(token)?;
        if !seen.insert(token_hash.clone()) {
            continue;
        }
        normalized.push(token_hash);
    }
    Ok(normalized)
}

fn normalize_agent_token_map(
    hashed_entries: HashMap<String, Vec<String>>,
    legacy_entries: HashMap<String, Vec<String>>,
) -> CodexResult<HashMap<String, Vec<String>>> {
    let mut normalized = HashMap::new();

    for (agent_name, tokens) in hashed_entries.into_iter().chain(legacy_entries) {
        let agent_name = normalize_agent_name(&agent_name)?;
        let tokens = normalize_token_entries(tokens)?;
        if tokens.is_empty() {
            continue;
        }
        normalized
            .entry(agent_name)
            .or_insert_with(Vec::new)
            .extend(tokens);
    }

    for tokens in normalized.values_mut() {
        *tokens = normalize_token_entries(std::mem::take(tokens))?;
    }

    Ok(normalized)
}

fn ensure_unique_active_token(
    seen: &mut HashMap<String, Option<String>>,
    agent_name: Option<&String>,
    token: &str,
) -> CodexResult<()> {
    let owner = agent_name.cloned();
    if let Some(existing_owner) = seen.insert(token.to_string(), owner.clone())
        && existing_owner != owner
    {
        let existing_label = existing_owner.unwrap_or_else(|| "shared_tokens".to_string());
        let new_label = owner.unwrap_or_else(|| "shared_tokens".to_string());
        return Err(CodexError::Coordination(format!(
            "duplicate active auth token assigned to `{existing_label}` and `{new_label}`"
        )));
    }
    Ok(())
}

fn config_path_exists(path: &Path) -> bool {
    path.exists() || backup_path(path).exists()
}

fn remove_token_hash(tokens: &mut Vec<String>, token_hash: &str) -> bool {
    let before = tokens.len();
    tokens.retain(|entry| entry != token_hash);
    tokens.len() != before
}

fn generate_token(label: &str) -> String {
    let prefix = sanitize_label(label);
    let entropy = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
    format!("nex_{prefix}_{entropy}")
}

fn sanitize_label(label: &str) -> String {
    let mut sanitized = String::new();
    let mut previous_dash = false;
    for ch in label.chars() {
        if ch.is_ascii_alphanumeric() {
            sanitized.push(ch.to_ascii_lowercase());
            previous_dash = false;
        } else if !previous_dash {
            sanitized.push('-');
            previous_dash = true;
        }
    }

    let trimmed = sanitized.trim_matches('-');
    if trimmed.is_empty() {
        "token".to_string()
    } else {
        trimmed.to_string()
    }
}

pub(crate) fn canonicalize_token_secret(token: &str) -> CodexResult<String> {
    let token = token.trim();
    if token.is_empty() {
        return Err(CodexError::Coordination(
            "token must not be empty".to_string(),
        ));
    }

    if let Some(hash) = token.strip_prefix(AUTH_HASH_PREFIX) {
        return normalize_token_hash(hash);
    }

    Ok(hash_token(token))
}

pub(crate) fn hash_token(token: &str) -> String {
    format!(
        "{AUTH_HASH_PREFIX}{}",
        blake3::hash(token.as_bytes()).to_hex()
    )
}

fn normalize_token_hash(hash: &str) -> CodexResult<String> {
    let hash = hash.trim().to_ascii_lowercase();
    if hash.len() != 64 || !hash.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return Err(CodexError::Coordination(
            "invalid hashed auth token; expected blake3: followed by 64 hex characters".to_string(),
        ));
    }
    Ok(format!("{AUTH_HASH_PREFIX}{hash}"))
}

const fn current_auth_config_version() -> u32 {
    CURRENT_AUTH_CONFIG_VERSION
}

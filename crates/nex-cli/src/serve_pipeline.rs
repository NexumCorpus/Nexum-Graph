//! Async coordination server pipeline for `nex serve`.

use crate::audit_pipeline::{AuditRecordInput, append_audit_record, default_audit_log_path};
use crate::auth_pipeline::{
    AuthConfigFile, AuthConfigMode, ENV_SERVER_AUTH_CONFIG, canonicalize_token_secret,
    discover_repo_auth_config_path, env_auth_config_path, hash_token,
    load_auth_config as load_auth_config_file,
};
use axum::extract::Request;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Extension, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::Utc;
use nex_coord::{CoordEvent, CoordinationService, GraphQuery, IntentPayload, IntentResult};
use nex_core::{CodexError, CodexResult};
use nex_eventlog::{EventLog, Mutation, SemanticEvent};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::{Mutex, broadcast, watch};
use tokio::task::JoinHandle;
use uuid::Uuid;

const ENV_SERVER_AUTH_TOKEN: &str = "NEX_SERVER_AUTH_TOKEN";
const ENV_SERVER_AGENT_TOKENS: &str = "NEX_SERVER_AGENT_TOKENS";

#[derive(Clone)]
struct AppState {
    service: Arc<Mutex<CoordinationService>>,
    events: broadcast::Sender<CoordEvent>,
    event_log: EventLog,
    audit_log: AuditLog,
}

#[derive(Debug, Clone, Default)]
pub struct ServeSecurity {
    auth_source: AuthSource,
    allow_insecure_remote: bool,
}

#[derive(Debug, Clone, Default)]
enum AuthSource {
    #[default]
    Disabled,
    Static(AuthSnapshot),
    ConfigFile(PathBuf),
}

#[derive(Debug, Clone, Default)]
struct AuthSnapshot {
    auth_mode: AuthMode,
    revoked_tokens: HashSet<String>,
}

#[derive(Debug, Clone, Default)]
enum AuthMode {
    #[default]
    Disabled,
    SharedTokens(HashSet<String>),
    AgentTokens(HashMap<String, String>),
}

#[derive(Debug, Clone, Default)]
struct AuthContext {
    authenticated_agent: Option<String>,
}

#[derive(Clone)]
struct AuthState {
    security: ServeSecurity,
    audit_log: AuditLog,
}

#[derive(Clone)]
struct AuditLog {
    path: PathBuf,
    gate: Arc<Mutex<()>>,
}

impl ServeSecurity {
    pub fn resolve_for_repo(
        repo_path: &Path,
        bearer_token: Option<String>,
        agent_tokens: Vec<String>,
        auth_config: Option<PathBuf>,
        allow_insecure_remote: bool,
    ) -> CodexResult<Self> {
        let auth_config = auth_config.or_else(env_auth_config_path);
        let token = bearer_token
            .or_else(|| std::env::var(ENV_SERVER_AUTH_TOKEN).ok())
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        let static_auth = static_auth_snapshot(
            token,
            agent_tokens
                .into_iter()
                .chain(
                    std::env::var(ENV_SERVER_AGENT_TOKENS)
                        .ok()
                        .map(|value| split_agent_token_env(&value))
                        .unwrap_or_default(),
                )
                .collect(),
        )?;

        if auth_config.is_some() && static_auth.is_enabled() {
            return Err(CodexError::Coordination(format!(
                "server auth configuration is ambiguous; use either --auth-config / {ENV_SERVER_AUTH_CONFIG} or static bearer options, not both"
            )));
        }

        let auth_source = if let Some(path) = auth_config {
            AuthSource::ConfigFile(path)
        } else if static_auth.is_enabled() {
            AuthSource::Static(static_auth)
        } else if let Some(path) = discover_repo_auth_config_path(repo_path) {
            AuthSource::ConfigFile(path)
        } else {
            AuthSource::Disabled
        };

        Ok(Self {
            auth_source,
            allow_insecure_remote,
        })
    }

    pub fn bearer_token(token: impl Into<String>) -> Self {
        let token = token.into();
        Self {
            auth_source: AuthSource::Static(AuthSnapshot {
                auth_mode: AuthMode::SharedTokens(HashSet::from([hash_token(&token)])),
                revoked_tokens: HashSet::new(),
            }),
            allow_insecure_remote: false,
        }
    }

    pub fn agent_tokens<I, A, T>(entries: I) -> CodexResult<Self>
    where
        I: IntoIterator<Item = (A, T)>,
        A: Into<String>,
        T: Into<String>,
    {
        let mut token_to_agent = HashMap::new();
        for (agent, token) in entries {
            let agent = agent.into();
            let token = token.into();
            validate_agent_token_entry(&agent, &token)?;
            ensure_unique_agent_token_mapping(&mut token_to_agent, agent, hash_token(&token))?;
        }

        Ok(Self {
            auth_source: AuthSource::Static(AuthSnapshot {
                auth_mode: AuthMode::AgentTokens(token_to_agent),
                revoked_tokens: HashSet::new(),
            }),
            allow_insecure_remote: false,
        })
    }

    pub fn auth_config(path: impl AsRef<Path>) -> Self {
        Self {
            auth_source: AuthSource::ConfigFile(path.as_ref().to_path_buf()),
            allow_insecure_remote: false,
        }
    }

    fn validate_bind_addr(&self, bind_addr: SocketAddr) -> CodexResult<()> {
        if bind_addr.ip().is_loopback() || self.allow_insecure_remote || self.auth_enabled() {
            return Ok(());
        }

        Err(CodexError::Coordination(format!(
            "refusing to bind nex serve to non-loopback address `{bind_addr}` without bearer auth; pass --auth-token, set {ENV_SERVER_AUTH_TOKEN}, or use --allow-insecure-remote"
        )))
    }

    fn auth_enabled(&self) -> bool {
        !matches!(self.auth_source, AuthSource::Disabled)
    }

    fn authenticate(&self, headers: &HeaderMap) -> Result<AuthContext, ApiError> {
        self.current_auth()
            .map_err(ApiError::from)?
            .authenticate(headers)
    }

    fn startup_check(&self) -> CodexResult<()> {
        let _ = self.current_auth()?;
        Ok(())
    }

    fn current_auth(&self) -> CodexResult<AuthSnapshot> {
        match &self.auth_source {
            AuthSource::Disabled => Ok(AuthSnapshot::default()),
            AuthSource::Static(snapshot) => Ok(snapshot.clone()),
            AuthSource::ConfigFile(path) => load_auth_snapshot(path),
        }
    }

    fn auth_description(&self) -> CodexResult<String> {
        let auth = self.current_auth()?;
        let label = match auth.mode() {
            AuthConfigMode::Disabled => String::new(),
            AuthConfigMode::Shared => match &self.auth_source {
                AuthSource::ConfigFile(path) => {
                    format!(" (reloadable shared bearer auth from {})", path.display())
                }
                _ => " (bearer auth required)".to_string(),
            },
            AuthConfigMode::Agent => match &self.auth_source {
                AuthSource::ConfigFile(path) => {
                    format!(
                        " (reloadable per-agent bearer auth from {})",
                        path.display()
                    )
                }
                _ => " (per-agent bearer auth required)".to_string(),
            },
        };
        Ok(label)
    }
}

impl AuthSnapshot {
    fn is_enabled(&self) -> bool {
        !matches!(self.auth_mode, AuthMode::Disabled)
    }

    fn mode(&self) -> AuthConfigMode {
        match &self.auth_mode {
            AuthMode::Disabled => AuthConfigMode::Disabled,
            AuthMode::SharedTokens(_) => AuthConfigMode::Shared,
            AuthMode::AgentTokens(_) => AuthConfigMode::Agent,
        }
    }

    fn authenticate(&self, headers: &HeaderMap) -> Result<AuthContext, ApiError> {
        let Some(token_hash) = extract_bearer_token_hash(headers) else {
            return match &self.auth_mode {
                AuthMode::Disabled => Ok(AuthContext::default()),
                AuthMode::SharedTokens(_) => {
                    Err(ApiError::unauthorized("missing or invalid bearer token"))
                }
                AuthMode::AgentTokens(_) => {
                    Err(ApiError::unauthorized("missing or invalid agent token"))
                }
            };
        };

        match &self.auth_mode {
            AuthMode::Disabled => Ok(AuthContext::default()),
            AuthMode::SharedTokens(expected_tokens)
                if !self.revoked_tokens.contains(&token_hash)
                    && expected_tokens.contains(&token_hash) =>
            {
                Ok(AuthContext::default())
            }
            AuthMode::SharedTokens(_) => {
                Err(ApiError::unauthorized("missing or invalid bearer token"))
            }
            AuthMode::AgentTokens(token_to_agent) if !self.revoked_tokens.contains(&token_hash) => {
                match token_to_agent.get(&token_hash) {
                    Some(agent) => Ok(AuthContext {
                        authenticated_agent: Some(agent.clone()),
                    }),
                    None => Err(ApiError::unauthorized("missing or invalid agent token")),
                }
            }
            AuthMode::AgentTokens(_) => {
                Err(ApiError::unauthorized("missing or invalid agent token"))
            }
        }
    }
}

impl AuditLog {
    fn for_repo(repo_path: &Path) -> Self {
        Self {
            path: default_audit_log_path(repo_path),
            gate: Arc::new(Mutex::new(())),
        }
    }

    async fn record(&self, record: AuditRecordInput) -> CodexResult<()> {
        let _guard = self.gate.lock().await;
        append_audit_record(&self.path, record)
    }
}

/// Request body for `/intent/commit`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitRequest {
    /// Intent id to commit.
    pub intent_id: Uuid,
    /// Lock token returned by `declare`.
    pub lock_token: Uuid,
    /// Optional commit description override.
    pub description: Option<String>,
    /// Semantic mutations emitted by the agent.
    pub mutations: Vec<Mutation>,
    /// Optional causal parent event.
    pub parent_event: Option<Uuid>,
    /// Free-form event tags.
    pub tags: Vec<String>,
}

/// Response body for `/intent/commit`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitResponse {
    /// Committed intent id.
    pub intent_id: Uuid,
    /// Event id appended to the local event log.
    pub event_id: Uuid,
    /// Number of locks released by commit.
    pub released_locks: usize,
}

/// Request body for `/intent/abort`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AbortRequest {
    /// Intent id to abort.
    pub intent_id: Uuid,
    /// Lock token returned by `declare`.
    pub lock_token: Uuid,
}

/// Response body for `/intent/abort`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AbortResponse {
    /// Aborted intent id.
    pub intent_id: Uuid,
    /// Number of locks released by abort.
    pub released_locks: usize,
}

/// Running server handle used by tests and the CLI.
pub struct ServerHandle {
    local_addr: SocketAddr,
    shutdown: watch::Sender<bool>,
    join: JoinHandle<()>,
}

impl ServerHandle {
    /// Actual bound address, useful when binding to port 0 in tests.
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    /// Stop the server and wait for all tasks to exit.
    pub async fn shutdown(self) {
        let _ = self.shutdown.send(true);
        let _ = self.join.await;
    }
}

/// Start the coordination server and keep it alive until Ctrl-C.
pub async fn run_serve(
    repo_path: &Path,
    host: &str,
    port: u16,
    auth_token: Option<String>,
    agent_tokens: Vec<String>,
    auth_config: Option<PathBuf>,
    allow_insecure_remote: bool,
) -> CodexResult<()> {
    let bind_addr: SocketAddr = format!("{host}:{port}").parse().map_err(|err| {
        CodexError::Coordination(format!("invalid bind address {host}:{port}: {err}"))
    })?;
    let security = ServeSecurity::resolve_for_repo(
        repo_path,
        auth_token,
        agent_tokens,
        auth_config,
        allow_insecure_remote,
    )?;

    let handle = spawn_server_with_options(repo_path, bind_addr, security.clone()).await?;
    let auth_suffix = if security.auth_enabled() {
        security.auth_description()?
    } else if !handle.local_addr().ip().is_loopback() {
        " (insecure remote bind)".to_string()
    } else {
        String::new()
    };
    println!(
        "nex serve listening on http://{}{}",
        handle.local_addr(),
        auth_suffix
    );
    tokio::signal::ctrl_c().await?;
    handle.shutdown().await;
    Ok(())
}

/// Spawn the coordination server on the requested address.
pub async fn spawn_server(repo_path: &Path, bind_addr: SocketAddr) -> CodexResult<ServerHandle> {
    spawn_server_with_options(repo_path, bind_addr, ServeSecurity::default()).await
}

/// Spawn the coordination server on the requested address with explicit security options.
pub async fn spawn_server_with_options(
    repo_path: &Path,
    bind_addr: SocketAddr,
    security: ServeSecurity,
) -> CodexResult<ServerHandle> {
    security.validate_bind_addr(bind_addr)?;
    security.startup_check()?;

    let graph = crate::coordination_pipeline::build_graph_from_head(repo_path)?;
    let service = CoordinationService::new(graph);
    let (events, _) = broadcast::channel(128);
    let audit_log = AuditLog::for_repo(repo_path);
    let state = AppState {
        service: Arc::new(Mutex::new(service)),
        events,
        event_log: EventLog::for_repo(repo_path),
        audit_log: audit_log.clone(),
    };
    let auth_state = AuthState {
        security,
        audit_log,
    };

    let app = Router::new()
        .route("/intent/declare", post(declare_intent))
        .route("/intent/commit", post(commit_intent))
        .route("/intent/abort", post(abort_intent))
        .route("/graph/query", get(query_graph))
        .route("/locks", get(list_locks))
        .route("/events", get(events_socket))
        .layer(middleware::from_fn_with_state(
            auth_state,
            require_bearer_auth,
        ))
        .with_state(state.clone());

    let listener = TcpListener::bind(bind_addr).await?;
    let local_addr = listener.local_addr()?;
    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    let mut expiry_shutdown = shutdown_rx.clone();
    let expiry_state = state.clone();
    let expiry_task = tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_millis(250));
        loop {
            tokio::select! {
                _ = interval.tick() => {
                    let expired = {
                        let mut service = expiry_state.service.lock().await;
                        service.expire_stale()
                    };
                    if !expired.is_empty() {
                        let intent_ids = expired.into_iter().map(|intent| intent.intent_id).collect();
                        let _ = expiry_state.events.send(CoordEvent::LocksExpired { intent_ids });
                    }
                }
                changed = expiry_shutdown.changed() => {
                    if changed.is_err() || *expiry_shutdown.borrow() {
                        break;
                    }
                }
            }
        }
    });

    let mut server_shutdown = shutdown_rx.clone();
    let join = tokio::spawn(async move {
        let server = axum::serve(listener, app).with_graceful_shutdown(async move {
            let _ = server_shutdown.changed().await;
        });
        let _ = server.await;
        expiry_task.abort();
        let _ = expiry_task.await;
    });

    Ok(ServerHandle {
        local_addr,
        shutdown: shutdown_tx,
        join,
    })
}

async fn require_bearer_auth(
    State(auth_state): State<AuthState>,
    mut request: Request,
    next: Next,
) -> Result<Response, ApiError> {
    let method = request.method().to_string();
    let path = request.uri().path().to_string();
    let auth = match auth_state.security.authenticate(request.headers()) {
        Ok(auth) => auth,
        Err(error) => {
            let _ = auth_state
                .audit_log
                .record(AuditRecordInput {
                    timestamp: Utc::now(),
                    action: "auth".to_string(),
                    outcome: if matches!(error, ApiError::Unauthorized(_)) {
                        "unauthorized".to_string()
                    } else {
                        "error".to_string()
                    },
                    method: Some(method),
                    path: Some(path),
                    authenticated_agent: None,
                    claimed_agent: None,
                    intent_id: None,
                    detail: Some(error.message()),
                })
                .await;
            return Err(error);
        }
    };
    request.extensions_mut().insert(auth);
    Ok(next.run(request).await)
}

async fn declare_intent(
    Extension(auth): Extension<AuthContext>,
    State(state): State<AppState>,
    Json(mut payload): Json<IntentPayload>,
) -> Result<Json<IntentResult>, ApiError> {
    if let Err(error) = ensure_declared_agent_matches_auth(&auth, &payload) {
        state
            .audit_log
            .record(AuditRecordInput {
                timestamp: Utc::now(),
                action: "intent_declare".to_string(),
                outcome: "forbidden".to_string(),
                method: Some("POST".to_string()),
                path: Some("/intent/declare".to_string()),
                authenticated_agent: auth.authenticated_agent.clone(),
                claimed_agent: Some(payload.agent_id.clone()),
                intent_id: Some(payload.id),
                detail: Some(error.message()),
            })
            .await?;
        return Err(error);
    }
    if let Some(agent) = auth.authenticated_agent.clone() {
        payload.agent_id = agent;
    }

    let mut service = state.service.lock().await;
    let result = service.declare_intent(payload.clone())?;
    drop(service);

    match &result {
        IntentResult::Approved { expires, .. } => {
            let _ = state.events.send(CoordEvent::IntentDeclared {
                intent_id: payload.id,
                agent_id: payload.agent_id.clone(),
                description: payload.description.clone(),
                targets: payload.target_units.clone(),
                expires: *expires,
            });
        }
        IntentResult::Rejected { conflicts } => {
            let _ = state.events.send(CoordEvent::IntentRejected {
                intent_id: payload.id,
                agent_id: payload.agent_id.clone(),
                conflicts: conflicts.clone(),
            });
        }
        IntentResult::Queued { .. } => {}
    }

    let outcome = match &result {
        IntentResult::Approved { .. } => "approved",
        IntentResult::Rejected { .. } => "rejected",
        IntentResult::Queued { .. } => "queued",
    };
    state
        .audit_log
        .record(AuditRecordInput {
            timestamp: Utc::now(),
            action: "intent_declare".to_string(),
            outcome: outcome.to_string(),
            method: Some("POST".to_string()),
            path: Some("/intent/declare".to_string()),
            authenticated_agent: auth.authenticated_agent.clone(),
            claimed_agent: Some(payload.agent_id.clone()),
            intent_id: Some(payload.id),
            detail: Some(payload.description.clone()),
        })
        .await?;

    Ok(Json(result))
}

async fn commit_intent(
    Extension(auth): Extension<AuthContext>,
    State(state): State<AppState>,
    Json(request): Json<CommitRequest>,
) -> Result<Json<CommitResponse>, ApiError> {
    let context = {
        let mut service = state.service.lock().await;
        if let Err(error) = ensure_intent_owner_matches_auth(&auth, &service, request.intent_id) {
            let claimed_agent = service
                .intent_owner(request.intent_id)
                .map(ToString::to_string);
            drop(service);
            state
                .audit_log
                .record(AuditRecordInput {
                    timestamp: Utc::now(),
                    action: "intent_commit".to_string(),
                    outcome: "forbidden".to_string(),
                    method: Some("POST".to_string()),
                    path: Some("/intent/commit".to_string()),
                    authenticated_agent: auth.authenticated_agent.clone(),
                    claimed_agent,
                    intent_id: Some(request.intent_id),
                    detail: Some(error.message()),
                })
                .await?;
            return Err(error);
        }
        service.commit_intent(request.intent_id, request.lock_token)?
    };

    let event = SemanticEvent {
        id: Uuid::new_v4(),
        timestamp: Utc::now(),
        intent_id: context.intent_id,
        agent_id: context.agent_id.clone(),
        description: request.description.unwrap_or(context.description),
        mutations: request.mutations,
        parent_event: request.parent_event,
        tags: request.tags,
    };

    state.event_log.append(event.clone()).await?;
    let _ = state.events.send(CoordEvent::IntentCommitted {
        intent_id: context.intent_id,
        agent_id: context.agent_id.clone(),
        event_id: Some(event.id),
        released_locks: context.released_locks,
    });
    state
        .audit_log
        .record(AuditRecordInput {
            timestamp: Utc::now(),
            action: "intent_commit".to_string(),
            outcome: "committed".to_string(),
            method: Some("POST".to_string()),
            path: Some("/intent/commit".to_string()),
            authenticated_agent: auth.authenticated_agent.clone(),
            claimed_agent: Some(event.agent_id.clone()),
            intent_id: Some(context.intent_id),
            detail: Some(event.description.clone()),
        })
        .await?;

    Ok(Json(CommitResponse {
        intent_id: context.intent_id,
        event_id: event.id,
        released_locks: context.released_locks,
    }))
}

async fn abort_intent(
    Extension(auth): Extension<AuthContext>,
    State(state): State<AppState>,
    Json(request): Json<AbortRequest>,
) -> Result<Json<AbortResponse>, ApiError> {
    let context = {
        let mut service = state.service.lock().await;
        if let Err(error) = ensure_intent_owner_matches_auth(&auth, &service, request.intent_id) {
            let claimed_agent = service
                .intent_owner(request.intent_id)
                .map(ToString::to_string);
            drop(service);
            state
                .audit_log
                .record(AuditRecordInput {
                    timestamp: Utc::now(),
                    action: "intent_abort".to_string(),
                    outcome: "forbidden".to_string(),
                    method: Some("POST".to_string()),
                    path: Some("/intent/abort".to_string()),
                    authenticated_agent: auth.authenticated_agent.clone(),
                    claimed_agent,
                    intent_id: Some(request.intent_id),
                    detail: Some(error.message()),
                })
                .await?;
            return Err(error);
        }
        service.abort_intent(request.intent_id, request.lock_token)?
    };

    let _ = state.events.send(CoordEvent::IntentAborted {
        intent_id: context.intent_id,
        agent_id: context.agent_id.clone(),
        released_locks: context.released_locks,
    });
    state
        .audit_log
        .record(AuditRecordInput {
            timestamp: Utc::now(),
            action: "intent_abort".to_string(),
            outcome: "aborted".to_string(),
            method: Some("POST".to_string()),
            path: Some("/intent/abort".to_string()),
            authenticated_agent: auth.authenticated_agent.clone(),
            claimed_agent: Some(context.agent_id.clone()),
            intent_id: Some(context.intent_id),
            detail: None,
        })
        .await?;

    Ok(Json(AbortResponse {
        intent_id: context.intent_id,
        released_locks: context.released_locks,
    }))
}

async fn query_graph(
    State(state): State<AppState>,
    Query(query): Query<GraphQuery>,
) -> Result<Json<Vec<nex_core::SemanticUnit>>, ApiError> {
    let service = state.service.lock().await;
    Ok(Json(service.query_graph(&query)?))
}

async fn list_locks(State(state): State<AppState>) -> Json<Vec<nex_coord::LockEntry>> {
    let service = state.service.lock().await;
    Json(service.locks())
}

async fn events_socket(ws: WebSocketUpgrade, State(state): State<AppState>) -> Response {
    ws.on_upgrade(move |socket| stream_events(socket, state.events.subscribe()))
}

async fn stream_events(mut socket: WebSocket, mut receiver: broadcast::Receiver<CoordEvent>) {
    loop {
        match receiver.recv().await {
            Ok(event) => {
                let Ok(payload) = serde_json::to_string(&event) else {
                    continue;
                };
                if socket.send(Message::Text(payload.into())).await.is_err() {
                    break;
                }
            }
            Err(broadcast::error::RecvError::Lagged(_)) => continue,
            Err(broadcast::error::RecvError::Closed) => break,
        }
    }
}

fn extract_bearer_token(headers: &HeaderMap) -> Option<&str> {
    let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    value.strip_prefix("Bearer ")
}

fn extract_bearer_token_hash(headers: &HeaderMap) -> Option<String> {
    let token = extract_bearer_token(headers)?;
    canonicalize_token_secret(token).ok()
}

fn ensure_declared_agent_matches_auth(
    auth: &AuthContext,
    payload: &IntentPayload,
) -> Result<(), ApiError> {
    let Some(agent) = auth.authenticated_agent.as_deref() else {
        return Ok(());
    };

    if payload.agent_id == agent {
        return Ok(());
    }

    Err(ApiError::forbidden(format!(
        "authenticated agent `{agent}` cannot declare intent for `{}`",
        payload.agent_id
    )))
}

fn ensure_intent_owner_matches_auth(
    auth: &AuthContext,
    service: &CoordinationService,
    intent_id: Uuid,
) -> Result<(), ApiError> {
    let Some(agent) = auth.authenticated_agent.as_deref() else {
        return Ok(());
    };

    let Some(owner) = service.intent_owner(intent_id) else {
        return Ok(());
    };

    if owner == agent {
        return Ok(());
    }

    Err(ApiError::forbidden(format!(
        "authenticated agent `{agent}` cannot act on intent owned by `{owner}`"
    )))
}

fn static_auth_snapshot(
    bearer_token: Option<String>,
    agent_tokens: Vec<String>,
) -> CodexResult<AuthSnapshot> {
    let token_to_agent = parse_agent_token_entries(agent_tokens)?;
    if bearer_token.is_some() && !token_to_agent.is_empty() {
        return Err(CodexError::Coordination(format!(
            "server auth configuration is ambiguous; use either --auth-token / {ENV_SERVER_AUTH_TOKEN} or --agent-token / {ENV_SERVER_AGENT_TOKENS}, not both"
        )));
    }

    if !token_to_agent.is_empty() {
        return Ok(AuthSnapshot {
            auth_mode: AuthMode::AgentTokens(token_to_agent),
            revoked_tokens: HashSet::new(),
        });
    }

    if let Some(token) = bearer_token {
        return Ok(AuthSnapshot {
            auth_mode: AuthMode::SharedTokens(HashSet::from([hash_token(&token)])),
            revoked_tokens: HashSet::new(),
        });
    }

    Ok(AuthSnapshot::default())
}

fn load_auth_snapshot(path: &Path) -> CodexResult<AuthSnapshot> {
    let config: AuthConfigFile = load_auth_config_file(path)?.ok_or_else(|| {
        CodexError::Coordination(format!("auth config file not found: {}", path.display()))
    })?;
    auth_snapshot_from_config(config)
}

fn auth_snapshot_from_config(config: AuthConfigFile) -> CodexResult<AuthSnapshot> {
    let revoked_tokens: HashSet<String> = config.revoked_token_hashes.into_iter().collect();

    let shared_tokens: HashSet<String> = config.shared_token_hashes.into_iter().collect();

    let mut token_to_agent = HashMap::new();
    for (agent, tokens) in config.agent_token_hashes {
        for token in tokens {
            ensure_unique_agent_token_mapping(&mut token_to_agent, agent.clone(), token)?;
        }
    }

    if !shared_tokens.is_empty() && !token_to_agent.is_empty() {
        return Err(CodexError::Coordination(
            "auth config must use either shared_tokens or agent_tokens, not both".to_string(),
        ));
    }

    let auth_mode = if !token_to_agent.is_empty() {
        AuthMode::AgentTokens(token_to_agent)
    } else if !shared_tokens.is_empty() {
        AuthMode::SharedTokens(shared_tokens)
    } else {
        AuthMode::Disabled
    };

    Ok(AuthSnapshot {
        auth_mode,
        revoked_tokens,
    })
}

fn parse_agent_token_entries(entries: Vec<String>) -> CodexResult<HashMap<String, String>> {
    let mut token_to_agent = HashMap::new();
    for entry in entries {
        let trimmed = entry.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Some((agent, token)) = trimmed.split_once('=') else {
            return Err(CodexError::Coordination(format!(
                "invalid agent token `{trimmed}`; expected agent=token"
            )));
        };
        validate_agent_token_entry(agent.trim(), token.trim())?;
        ensure_unique_agent_token_mapping(
            &mut token_to_agent,
            agent.trim().to_string(),
            hash_token(token.trim()),
        )?;
    }
    Ok(token_to_agent)
}

fn split_agent_token_env(value: &str) -> Vec<String> {
    value
        .split([',', ';'])
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn validate_agent_token_entry(agent: &str, token: &str) -> CodexResult<()> {
    if agent.is_empty() || token.is_empty() {
        return Err(CodexError::Coordination(
            "agent token entries require non-empty agent and token".to_string(),
        ));
    }
    Ok(())
}

fn ensure_unique_agent_token_mapping(
    token_to_agent: &mut HashMap<String, String>,
    agent: String,
    token: String,
) -> CodexResult<()> {
    if let Some(existing_agent) = token_to_agent.insert(token.clone(), agent.clone())
        && existing_agent != agent
    {
        return Err(CodexError::Coordination(format!(
            "duplicate bearer token configured for agents `{existing_agent}` and `{agent}`"
        )));
    }
    Ok(())
}

enum ApiError {
    Codex(CodexError),
    Unauthorized(String),
    Forbidden(String),
}

impl From<CodexError> for ApiError {
    fn from(value: CodexError) -> Self {
        Self::Codex(value)
    }
}

impl ApiError {
    fn unauthorized(message: impl Into<String>) -> Self {
        Self::Unauthorized(message.into())
    }

    fn forbidden(message: impl Into<String>) -> Self {
        Self::Forbidden(message.into())
    }

    fn message(&self) -> String {
        match self {
            Self::Codex(error) => match error {
                CodexError::Git(message)
                | CodexError::Graph(message)
                | CodexError::Coordination(message) => message.clone(),
                CodexError::Parse { message, .. } => message.clone(),
                CodexError::Io(error) => error.to_string(),
                CodexError::Serialization(error) => error.to_string(),
            },
            Self::Unauthorized(message) | Self::Forbidden(message) => message.clone(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        match self {
            Self::Codex(error) => {
                let status = match error {
                    CodexError::Io(_) | CodexError::Serialization(_) => {
                        StatusCode::INTERNAL_SERVER_ERROR
                    }
                    CodexError::Git(_)
                    | CodexError::Parse { .. }
                    | CodexError::Graph(_)
                    | CodexError::Coordination(_) => StatusCode::BAD_REQUEST,
                };

                let body = serde_json::json!({
                    "error": error.to_string(),
                });
                (status, Json(body)).into_response()
            }
            Self::Unauthorized(message) => {
                let body = serde_json::json!({
                    "error": message,
                });
                (
                    StatusCode::UNAUTHORIZED,
                    [(header::WWW_AUTHENTICATE, "Bearer")],
                    Json(body),
                )
                    .into_response()
            }
            Self::Forbidden(message) => {
                let body = serde_json::json!({
                    "error": message,
                });
                (StatusCode::FORBIDDEN, Json(body)).into_response()
            }
        }
    }
}

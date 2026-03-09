use chrono::Utc;
use futures_util::StreamExt;
use nex_cli::auth_pipeline::init_auth_config;
use nex_cli::serve_pipeline::{
    AbortRequest, CommitRequest, ServeSecurity, spawn_server, spawn_server_with_options,
};
use nex_coord::{CoordEvent, IntentPayload, IntentResult, LockEntry, PlannedChange};
use nex_core::SemanticUnit;
use nex_eventlog::{Mutation, SemanticEvent};
use serde_json::Value;
use std::net::SocketAddr;
use std::path::Path;
use std::time::Duration;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::error::Error as WsError;
use uuid::Uuid;

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

fn declare_payload(unit: &SemanticUnit) -> IntentPayload {
    IntentPayload {
        id: Uuid::new_v4(),
        agent_id: "alice".to_string(),
        timestamp: Utc::now(),
        description: format!("edit {}", unit.name),
        target_units: vec![unit.id],
        estimated_changes: vec![PlannedChange::ModifyBody { unit: unit.id }],
        ttl: Duration::from_secs(30),
    }
}

fn bearer_request(url: &str, token: &str) -> tokio_tungstenite::tungstenite::http::Request<()> {
    let mut request = url.into_client_request().expect("build ws request");
    request.headers_mut().insert(
        "Authorization",
        format!("Bearer {token}").parse().expect("auth header"),
    );
    request
}

fn write_auth_config(path: &Path, body: serde_json::Value) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create auth config dir");
    }
    std::fs::write(
        path,
        serde_json::to_vec_pretty(&body).expect("serialize auth config"),
    )
    .expect("write auth config");
}

fn read_audit_log(path: &Path) -> Vec<Value> {
    std::fs::read_to_string(path)
        .expect("read audit log")
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).expect("parse audit record"))
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn serve_exposes_declare_locks_commit_and_abort() {
    let (_dir, repo) = setup_repo();
    let repo_path = repo.workdir().unwrap().to_path_buf();
    let server = spawn_server(&repo_path, "127.0.0.1:0".parse::<SocketAddr>().unwrap())
        .await
        .unwrap();
    let base = format!("http://{}", server.local_addr());
    let client = reqwest::Client::new();

    let process_request: Vec<SemanticUnit> = client
        .get(format!("{base}/graph/query"))
        .query(&[("kind", "units_named"), ("value", "processRequest")])
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(process_request.len(), 1);

    let declare = declare_payload(&process_request[0]);
    let declare_result: IntentResult = client
        .post(format!("{base}/intent/declare"))
        .json(&declare)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();

    let lock_token = match declare_result {
        IntentResult::Approved { lock_token, .. } => lock_token,
        other => panic!("expected approval, got {other:?}"),
    };

    let locks: Vec<LockEntry> = client
        .get(format!("{base}/locks"))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(locks.len(), 1);
    assert_eq!(locks[0].holder, "alice");
    assert_eq!(locks[0].target_name, "processRequest");

    let mut after = process_request[0].clone();
    after.body_hash += 1;
    let commit_response: nex_cli::serve_pipeline::CommitResponse = client
        .post(format!("{base}/intent/commit"))
        .json(&CommitRequest {
            intent_id: declare.id,
            lock_token,
            description: Some("commit processRequest".to_string()),
            mutations: vec![Mutation::ModifyUnit {
                id: process_request[0].id,
                before: process_request[0].clone(),
                after: after.clone(),
            }],
            parent_event: None,
            tags: vec!["feature:test".to_string()],
        })
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(commit_response.intent_id, declare.id);

    let events_path = repo_path.join(".nex").join("events.json");
    let logged: Vec<SemanticEvent> =
        serde_json::from_str(&std::fs::read_to_string(events_path).unwrap()).unwrap();
    assert_eq!(logged.len(), 1);
    assert_eq!(logged[0].id, commit_response.event_id);

    let locks_after_commit: Vec<LockEntry> = client
        .get(format!("{base}/locks"))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(locks_after_commit.is_empty());

    let validate: Vec<SemanticUnit> = client
        .get(format!("{base}/graph/query"))
        .query(&[("kind", "units_named"), ("value", "validate")])
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let abort_declare = declare_payload(&validate[0]);
    let abort_result: IntentResult = client
        .post(format!("{base}/intent/declare"))
        .json(&abort_declare)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let abort_token = match abort_result {
        IntentResult::Approved { lock_token, .. } => lock_token,
        other => panic!("expected approval, got {other:?}"),
    };

    let abort_response: nex_cli::serve_pipeline::AbortResponse = client
        .post(format!("{base}/intent/abort"))
        .json(&AbortRequest {
            intent_id: abort_declare.id,
            lock_token: abort_token,
        })
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(abort_response.intent_id, abort_declare.id);

    server.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn serve_streams_coordination_events_over_websocket() {
    let (_dir, repo) = setup_repo();
    let repo_path = repo.workdir().unwrap().to_path_buf();
    let server = spawn_server(&repo_path, "127.0.0.1:0".parse::<SocketAddr>().unwrap())
        .await
        .unwrap();
    let http_base = format!("http://{}", server.local_addr());
    let ws_base = format!("ws://{}/events", server.local_addr());
    let client = reqwest::Client::new();

    let (mut socket, _) = connect_async(&ws_base).await.unwrap();

    let validate: Vec<SemanticUnit> = client
        .get(format!("{http_base}/graph/query"))
        .query(&[("kind", "units_named"), ("value", "validate")])
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();

    let declare = declare_payload(&validate[0]);
    let result: IntentResult = client
        .post(format!("{http_base}/intent/declare"))
        .json(&declare)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(matches!(result, IntentResult::Approved { .. }));

    let message = tokio::time::timeout(Duration::from_secs(5), socket.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let text = message.into_text().unwrap();
    let event: CoordEvent = serde_json::from_str(&text).unwrap();
    match event {
        CoordEvent::IntentDeclared {
            intent_id,
            agent_id,
            ..
        } => {
            assert_eq!(intent_id, declare.id);
            assert_eq!(agent_id, "alice");
        }
        other => panic!("expected intent declared event, got {other:?}"),
    }

    server.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn serve_rejects_remote_bind_without_auth() {
    let (_dir, repo) = setup_repo();
    let repo_path = repo.workdir().unwrap().to_path_buf();

    let error = match spawn_server_with_options(
        &repo_path,
        "0.0.0.0:0".parse::<SocketAddr>().unwrap(),
        ServeSecurity::default(),
    )
    .await
    {
        Ok(handle) => {
            handle.shutdown().await;
            panic!("remote bind without auth should fail");
        }
        Err(error) => error,
    };

    assert!(
        error
            .to_string()
            .contains("refusing to bind nex serve to non-loopback address"),
        "unexpected error: {error}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn serve_requires_bearer_token_for_http_routes() {
    let (_dir, repo) = setup_repo();
    let repo_path = repo.workdir().unwrap().to_path_buf();
    let server = spawn_server_with_options(
        &repo_path,
        "127.0.0.1:0".parse::<SocketAddr>().unwrap(),
        ServeSecurity::bearer_token("topsecret"),
    )
    .await
    .unwrap();
    let base = format!("http://{}", server.local_addr());
    let client = reqwest::Client::new();

    let unauthorized = client.get(format!("{base}/locks")).send().await.unwrap();
    assert_eq!(unauthorized.status(), reqwest::StatusCode::UNAUTHORIZED);

    let authorized = client
        .get(format!("{base}/locks"))
        .bearer_auth("topsecret")
        .send()
        .await
        .unwrap();
    assert_eq!(authorized.status(), reqwest::StatusCode::OK);

    server.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn serve_requires_bearer_token_for_websocket_stream() {
    let (_dir, repo) = setup_repo();
    let repo_path = repo.workdir().unwrap().to_path_buf();
    let server = spawn_server_with_options(
        &repo_path,
        "127.0.0.1:0".parse::<SocketAddr>().unwrap(),
        ServeSecurity::bearer_token("topsecret"),
    )
    .await
    .unwrap();
    let http_base = format!("http://{}", server.local_addr());
    let ws_base = format!("ws://{}/events", server.local_addr());
    let client = reqwest::Client::new();

    let unauthorized = connect_async(&ws_base)
        .await
        .expect_err("ws should require auth");
    match unauthorized {
        WsError::Http(response) => assert_eq!(response.status(), reqwest::StatusCode::UNAUTHORIZED),
        other => panic!("expected unauthorized websocket response, got {other:?}"),
    }

    let (mut socket, _) = connect_async(bearer_request(&ws_base, "topsecret"))
        .await
        .unwrap();

    let validate: Vec<SemanticUnit> = client
        .get(format!("{http_base}/graph/query"))
        .bearer_auth("topsecret")
        .query(&[("kind", "units_named"), ("value", "validate")])
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();

    let declare = declare_payload(&validate[0]);
    let result: IntentResult = client
        .post(format!("{http_base}/intent/declare"))
        .bearer_auth("topsecret")
        .json(&declare)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(matches!(result, IntentResult::Approved { .. }));

    let message = tokio::time::timeout(Duration::from_secs(5), socket.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let text = message.into_text().unwrap();
    let event: CoordEvent = serde_json::from_str(&text).unwrap();
    match event {
        CoordEvent::IntentDeclared { intent_id, .. } => assert_eq!(intent_id, declare.id),
        other => panic!("expected intent declared event, got {other:?}"),
    }

    let _ = socket.close(None).await;
    server.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn serve_agent_tokens_reject_mismatched_declared_agent() {
    let (_dir, repo) = setup_repo();
    let repo_path = repo.workdir().unwrap().to_path_buf();
    let server = spawn_server_with_options(
        &repo_path,
        "127.0.0.1:0".parse::<SocketAddr>().unwrap(),
        ServeSecurity::agent_tokens([("alice", "alice-secret"), ("bob", "bob-secret")]).unwrap(),
    )
    .await
    .unwrap();
    let base = format!("http://{}", server.local_addr());
    let client = reqwest::Client::new();

    let validate: Vec<SemanticUnit> = client
        .get(format!("{base}/graph/query"))
        .bearer_auth("alice-secret")
        .query(&[("kind", "units_named"), ("value", "validate")])
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();

    let mut declare = declare_payload(&validate[0]);
    declare.agent_id = "mallory".to_string();

    let response = client
        .post(format!("{base}/intent/declare"))
        .bearer_auth("alice-secret")
        .json(&declare)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::FORBIDDEN);

    server.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn serve_agent_tokens_prevent_cross_agent_commit() {
    let (_dir, repo) = setup_repo();
    let repo_path = repo.workdir().unwrap().to_path_buf();
    let server = spawn_server_with_options(
        &repo_path,
        "127.0.0.1:0".parse::<SocketAddr>().unwrap(),
        ServeSecurity::agent_tokens([("alice", "alice-secret"), ("bob", "bob-secret")]).unwrap(),
    )
    .await
    .unwrap();
    let base = format!("http://{}", server.local_addr());
    let client = reqwest::Client::new();

    let process_request: Vec<SemanticUnit> = client
        .get(format!("{base}/graph/query"))
        .bearer_auth("alice-secret")
        .query(&[("kind", "units_named"), ("value", "processRequest")])
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();

    let declare = declare_payload(&process_request[0]);
    let declare_result: IntentResult = client
        .post(format!("{base}/intent/declare"))
        .bearer_auth("alice-secret")
        .json(&declare)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let lock_token = match declare_result {
        IntentResult::Approved { lock_token, .. } => lock_token,
        other => panic!("expected approval, got {other:?}"),
    };

    let response = client
        .post(format!("{base}/intent/commit"))
        .bearer_auth("bob-secret")
        .json(&CommitRequest {
            intent_id: declare.id,
            lock_token,
            description: Some("bob should not commit".to_string()),
            mutations: Vec::new(),
            parent_event: None,
            tags: Vec::new(),
        })
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::FORBIDDEN);

    let locks: Vec<LockEntry> = client
        .get(format!("{base}/locks"))
        .bearer_auth("alice-secret")
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(locks.len(), 1);
    assert_eq!(locks[0].holder, "alice");

    server.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn serve_auto_discovers_repo_auth_config() {
    let (_dir, repo) = setup_repo();
    let repo_path = repo.workdir().unwrap().to_path_buf();
    let auth_path = repo_path.join(".nex").join("server-auth.json");
    write_auth_config(
        &auth_path,
        serde_json::json!({
            "agent_tokens": {
                "alice": ["alice-secret"]
            }
        }),
    );

    let security = ServeSecurity::resolve_for_repo(&repo_path, None, Vec::new(), None, false)
        .expect("resolve repo auth config");
    let server = spawn_server_with_options(
        &repo_path,
        "127.0.0.1:0".parse::<SocketAddr>().unwrap(),
        security,
    )
    .await
    .unwrap();
    let base = format!("http://{}", server.local_addr());
    let client = reqwest::Client::new();

    let unauthorized = client.get(format!("{base}/locks")).send().await.unwrap();
    assert_eq!(unauthorized.status(), reqwest::StatusCode::UNAUTHORIZED);

    let authorized = client
        .get(format!("{base}/locks"))
        .bearer_auth("alice-secret")
        .send()
        .await
        .unwrap();
    assert_eq!(authorized.status(), reqwest::StatusCode::OK);

    server.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn serve_accepts_hash_only_auth_config_written_by_cli() {
    let (_dir, repo) = setup_repo();
    let repo_path = repo.workdir().unwrap().to_path_buf();
    let init = init_auth_config(&repo_path, None, &["alice".to_string()], false, false)
        .expect("init hashed auth config");
    let token = init.issued[0].token.clone();
    let auth_body = std::fs::read_to_string(repo_path.join(".nex").join("server-auth.json"))
        .expect("read hashed auth config");
    assert!(!auth_body.contains(&token));

    let security = ServeSecurity::resolve_for_repo(&repo_path, None, Vec::new(), None, false)
        .expect("resolve hashed auth config");
    let server = spawn_server_with_options(
        &repo_path,
        "127.0.0.1:0".parse::<SocketAddr>().unwrap(),
        security,
    )
    .await
    .unwrap();
    let base = format!("http://{}", server.local_addr());
    let client = reqwest::Client::new();

    let authorized = client
        .get(format!("{base}/locks"))
        .bearer_auth(token)
        .send()
        .await
        .unwrap();
    assert_eq!(authorized.status(), reqwest::StatusCode::OK);

    server.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn serve_reloadable_auth_config_rotates_tokens_without_restart() {
    let (_dir, repo) = setup_repo();
    let repo_path = repo.workdir().unwrap().to_path_buf();
    let auth_path = repo_path.join(".nex").join("server-auth.json");
    write_auth_config(
        &auth_path,
        serde_json::json!({
            "agent_tokens": {
                "alice": ["alice-old"]
            }
        }),
    );

    let server = spawn_server_with_options(
        &repo_path,
        "127.0.0.1:0".parse::<SocketAddr>().unwrap(),
        ServeSecurity::auth_config(&auth_path),
    )
    .await
    .unwrap();
    let base = format!("http://{}", server.local_addr());
    let client = reqwest::Client::new();

    let old_ok = client
        .get(format!("{base}/locks"))
        .bearer_auth("alice-old")
        .send()
        .await
        .unwrap();
    assert_eq!(old_ok.status(), reqwest::StatusCode::OK);

    write_auth_config(
        &auth_path,
        serde_json::json!({
            "agent_tokens": {
                "alice": ["alice-new"]
            },
            "revoked_tokens": ["alice-old"]
        }),
    );

    let old_denied = client
        .get(format!("{base}/locks"))
        .bearer_auth("alice-old")
        .send()
        .await
        .unwrap();
    assert_eq!(old_denied.status(), reqwest::StatusCode::UNAUTHORIZED);

    let new_ok = client
        .get(format!("{base}/locks"))
        .bearer_auth("alice-new")
        .send()
        .await
        .unwrap();
    assert_eq!(new_ok.status(), reqwest::StatusCode::OK);

    server.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn serve_writes_append_only_audit_log_for_auth_sensitive_actions() {
    let (_dir, repo) = setup_repo();
    let repo_path = repo.workdir().unwrap().to_path_buf();
    let server = spawn_server_with_options(
        &repo_path,
        "127.0.0.1:0".parse::<SocketAddr>().unwrap(),
        ServeSecurity::agent_tokens([("alice", "alice-secret")]).unwrap(),
    )
    .await
    .unwrap();
    let base = format!("http://{}", server.local_addr());
    let client = reqwest::Client::new();

    let unauthorized = client.get(format!("{base}/locks")).send().await.unwrap();
    assert_eq!(unauthorized.status(), reqwest::StatusCode::UNAUTHORIZED);

    let validate: Vec<SemanticUnit> = client
        .get(format!("{base}/graph/query"))
        .bearer_auth("alice-secret")
        .query(&[("kind", "units_named"), ("value", "validate")])
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();

    let declare = declare_payload(&validate[0]);
    let response = client
        .post(format!("{base}/intent/declare"))
        .bearer_auth("alice-secret")
        .json(&declare)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::OK);

    let audit_log = read_audit_log(&repo_path.join(".nex").join("server-audit.jsonl"));
    assert!(audit_log.len() >= 2);
    assert!(audit_log.iter().any(|record| {
        record.get("action") == Some(&Value::String("auth".to_string()))
            && record.get("outcome") == Some(&Value::String("unauthorized".to_string()))
            && record.get("path") == Some(&Value::String("/locks".to_string()))
    }));
    assert!(audit_log.iter().any(|record| {
        record.get("action") == Some(&Value::String("intent_declare".to_string()))
            && record.get("outcome") == Some(&Value::String("approved".to_string()))
            && record.get("claimed_agent") == Some(&Value::String("alice".to_string()))
    }));

    server.shutdown().await;
}

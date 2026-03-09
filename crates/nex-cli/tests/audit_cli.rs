use chrono::Utc;
use nex_cli::audit_pipeline::{default_audit_log_path, verify_audit_log};
use nex_cli::serve_pipeline::{ServeSecurity, spawn_server_with_options};
use nex_coord::{IntentPayload, IntentResult, PlannedChange};
use nex_core::SemanticUnit;
use serde_json::json;
use std::net::SocketAddr;
use std::path::Path;
use std::time::Duration;
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

async fn generate_server_audit(repo_path: &Path, token: &str) {
    let server = spawn_server_with_options(
        repo_path,
        "127.0.0.1:0".parse::<SocketAddr>().unwrap(),
        ServeSecurity::agent_tokens([("alice", token)]).unwrap(),
    )
    .await
    .unwrap();
    let base = format!("http://{}", server.local_addr());
    let client = reqwest::Client::new();

    let unauthorized = client.get(format!("{base}/locks")).send().await.unwrap();
    assert_eq!(unauthorized.status(), reqwest::StatusCode::UNAUTHORIZED);

    let validate: Vec<SemanticUnit> = client
        .get(format!("{base}/graph/query"))
        .bearer_auth(token)
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
    let response: IntentResult = client
        .post(format!("{base}/intent/declare"))
        .bearer_auth(token)
        .json(&declare)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(matches!(response, IntentResult::Approved { .. }));

    server.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn audit_verify_reports_valid_hash_chained_server_log() {
    let (_dir, repo) = setup_repo();
    let repo_path = repo.workdir().unwrap();
    generate_server_audit(repo_path, "alice-secret").await;

    let report = verify_audit_log(repo_path, None).expect("verify audit log");
    assert!(report.valid);
    assert!(report.anchored);
    assert!(report.record_count >= 2);

    let audit_body =
        std::fs::read_to_string(default_audit_log_path(repo_path)).expect("read audit log");
    assert!(audit_body.contains("\"entry_hash\""));
    assert!(audit_body.contains("\"prev_hash\""));
    assert!(report.head_path.exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn audit_verify_detects_tampered_record() {
    let (_dir, repo) = setup_repo();
    let repo_path = repo.workdir().unwrap();
    generate_server_audit(repo_path, "alice-secret").await;

    let log_path = default_audit_log_path(repo_path);
    let body = std::fs::read_to_string(&log_path).expect("read audit log");
    let tampered = body.replacen("\"outcome\":\"unauthorized\"", "\"outcome\":\"forged\"", 1);
    std::fs::write(&log_path, tampered).expect("write tampered audit log");

    let report = verify_audit_log(repo_path, None).expect("verify tampered audit log");
    assert!(!report.valid);
    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue.kind == "hash_mismatch")
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn audit_verify_detects_truncated_log_against_head_anchor() {
    let (_dir, repo) = setup_repo();
    let repo_path = repo.workdir().unwrap();
    generate_server_audit(repo_path, "alice-secret").await;

    let log_path = default_audit_log_path(repo_path);
    let mut lines = std::fs::read_to_string(&log_path)
        .expect("read audit log")
        .lines()
        .map(str::to_string)
        .collect::<Vec<_>>();
    lines.pop();
    std::fs::write(&log_path, format!("{}\n", lines.join("\n"))).expect("truncate audit log");

    let report = verify_audit_log(repo_path, None).expect("verify truncated audit log");
    assert!(!report.valid);
    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue.kind == "head_mismatch")
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn audit_append_migrates_legacy_records_on_next_write() {
    let (_dir, repo) = setup_repo();
    let repo_path = repo.workdir().unwrap();
    let log_path = default_audit_log_path(repo_path);
    std::fs::create_dir_all(log_path.parent().expect("audit dir")).expect("mkdir");
    std::fs::write(
        &log_path,
        format!(
            "{}\n",
            serde_json::to_string(&json!({
                "timestamp": Utc::now(),
                "action": "auth",
                "outcome": "unauthorized",
                "method": "GET",
                "path": "/locks",
                "authenticated_agent": null,
                "claimed_agent": null,
                "intent_id": null,
                "detail": "missing or invalid bearer token"
            }))
            .expect("serialize legacy record")
        ),
    )
    .expect("write legacy log");

    generate_server_audit(repo_path, "alice-secret").await;

    let report = verify_audit_log(repo_path, None).expect("verify migrated log");
    assert!(report.valid);
    assert!(report.record_count >= 3);

    let audit_body = std::fs::read_to_string(&log_path).expect("read migrated log");
    assert!(audit_body.contains("\"entry_hash\""));
}

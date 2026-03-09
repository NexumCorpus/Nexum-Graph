use nex_cli::auth_pipeline::{
    AuthConfigMode, AuthIssueTarget, auth_status, default_auth_config_path, init_auth_config,
    issue_auth_token, revoke_auth_token,
};
use serde_json::Value;

#[test]
fn auth_init_status_issue_and_revoke_round_trip() {
    let dir = tempfile::tempdir().expect("tempdir");

    let init = init_auth_config(
        dir.path(),
        None,
        &["alice".to_string(), "bob".to_string()],
        false,
        false,
    )
    .expect("init auth config");
    assert_eq!(init.mode, AuthConfigMode::Agent);
    assert_eq!(init.issued.len(), 2);
    assert!(default_auth_config_path(dir.path()).exists());

    let status = auth_status(dir.path(), None).expect("status after init");
    assert!(status.exists);
    assert_eq!(status.mode, AuthConfigMode::Agent);
    assert_eq!(status.agents.len(), 2);

    let issued = issue_auth_token(
        dir.path(),
        None,
        AuthIssueTarget::Agent("alice".to_string()),
    )
    .expect("issue alice token");
    assert_eq!(issued.issued.agent_name.as_deref(), Some("alice"));
    assert_eq!(issued.active_token_count, 3);

    let revoked = revoke_auth_token(dir.path(), None, &issued.issued.token).expect("revoke token");
    assert!(revoked.removed);
    assert_eq!(revoked.affected_agent.as_deref(), Some("alice"));
    assert_eq!(revoked.revoked_token_count, 1);

    let status = auth_status(dir.path(), None).expect("status after revoke");
    let alice = status
        .agents
        .iter()
        .find(|agent| agent.agent_name == "alice")
        .expect("alice status");
    let bob = status
        .agents
        .iter()
        .find(|agent| agent.agent_name == "bob")
        .expect("bob status");
    assert_eq!(alice.active_tokens, 1);
    assert_eq!(bob.active_tokens, 1);
    assert_eq!(status.revoked_token_count, 1);
}

#[test]
fn auth_shared_mode_rejects_agent_issue() {
    let dir = tempfile::tempdir().expect("tempdir");

    let init = init_auth_config(dir.path(), None, &[], true, false).expect("init shared auth");
    assert_eq!(init.mode, AuthConfigMode::Shared);
    assert_eq!(init.issued.len(), 1);

    let second = issue_auth_token(dir.path(), None, AuthIssueTarget::Shared)
        .expect("issue second shared token");
    assert_eq!(second.mode, AuthConfigMode::Shared);
    assert_eq!(second.active_token_count, 2);

    let error = issue_auth_token(
        dir.path(),
        None,
        AuthIssueTarget::Agent("alice".to_string()),
    )
    .expect_err("mixed auth modes should fail");
    assert!(
        error.to_string().contains("shared-token mode"),
        "unexpected error: {error}"
    );
}

#[test]
fn auth_init_requires_force_to_replace_existing_config() {
    let dir = tempfile::tempdir().expect("tempdir");

    init_auth_config(dir.path(), None, &["alice".to_string()], false, false).expect("first init");

    let error = init_auth_config(dir.path(), None, &["bob".to_string()], false, false)
        .expect_err("second init without force should fail");
    assert!(
        error.to_string().contains("--force"),
        "unexpected error: {error}"
    );

    let replaced = init_auth_config(dir.path(), None, &["bob".to_string()], false, true)
        .expect("replace auth config");
    assert!(replaced.replaced_existing);

    let status = auth_status(dir.path(), None).expect("status after replace");
    assert_eq!(status.agents.len(), 1);
    assert_eq!(status.agents[0].agent_name, "bob");
}

#[test]
fn auth_commands_support_explicit_config_paths() {
    let dir = tempfile::tempdir().expect("tempdir");
    let custom_path = dir.path().join("ops").join("auth.json");

    init_auth_config(
        dir.path(),
        Some(custom_path.clone()),
        &["alice".to_string()],
        false,
        false,
    )
    .expect("init custom auth config");

    let status = auth_status(dir.path(), Some(custom_path.clone())).expect("status");
    assert_eq!(status.path, custom_path);
    assert!(status.exists);
    assert!(!default_auth_config_path(dir.path()).exists());
}

#[test]
fn auth_init_persists_hashes_not_plaintext_tokens() {
    let dir = tempfile::tempdir().expect("tempdir");

    let init = init_auth_config(dir.path(), None, &["alice".to_string()], false, false)
        .expect("init auth config");
    let config_path = default_auth_config_path(dir.path());
    let body = std::fs::read_to_string(&config_path).expect("read auth config");
    let json: Value = serde_json::from_str(&body).expect("parse auth config json");

    assert!(!body.contains(&init.issued[0].token));
    assert!(json.get("agent_tokens").is_none());
    assert!(json.get("agent_token_hashes").is_some());
    assert_eq!(json.get("format_version"), Some(&Value::from(2_u64)));
}

#[test]
fn auth_issue_rewrites_legacy_plaintext_config_to_hash_only_storage() {
    let dir = tempfile::tempdir().expect("tempdir");
    let config_path = default_auth_config_path(dir.path());
    std::fs::create_dir_all(config_path.parent().expect("auth config parent")).expect("mkdir");
    std::fs::write(
        &config_path,
        serde_json::to_vec_pretty(&serde_json::json!({
            "agent_tokens": {
                "alice": ["legacy-secret"]
            }
        }))
        .expect("serialize legacy auth config"),
    )
    .expect("write legacy auth config");

    let issued = issue_auth_token(
        dir.path(),
        None,
        AuthIssueTarget::Agent("alice".to_string()),
    )
    .expect("issue token");
    let body = std::fs::read_to_string(&config_path).expect("read rewritten auth config");
    let json: Value = serde_json::from_str(&body).expect("parse rewritten auth config");

    assert!(!body.contains("legacy-secret"));
    assert!(!body.contains(&issued.issued.token));
    assert!(json.get("agent_tokens").is_none());
    assert!(json.get("agent_token_hashes").is_some());
}

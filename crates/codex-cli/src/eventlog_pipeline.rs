//! Eventlog pipeline: local event store -> list / rollback.

use codex_core::{CodexError, CodexResult, SemanticUnit};
use codex_eventlog::{EventLog, RollbackOutcome, SemanticEvent};
use std::path::{Path, PathBuf};
use uuid::Uuid;

fn event_log_path(repo_path: &Path) -> PathBuf {
    repo_path.join(".codex").join("events.json")
}

/// List semantic events, optionally filtered by intent id.
pub fn run_log(repo_path: &Path, intent_id: Option<&str>) -> CodexResult<Vec<SemanticEvent>> {
    let log = EventLog::new(event_log_path(repo_path));
    match intent_id {
        Some(intent_id) => {
            let parsed = parse_uuid(intent_id)?;
            log.events_for_intent(parsed)
        }
        None => log.list(),
    }
}

/// Generate and append a rollback event for the given intent id.
pub fn run_rollback(
    repo_path: &Path,
    intent_id: &str,
    agent_name: &str,
) -> CodexResult<RollbackOutcome> {
    let parsed = parse_uuid(intent_id)?;
    let log = EventLog::new(event_log_path(repo_path));
    log.rollback(parsed, agent_name, &format!("rollback {intent_id}"))
}

/// Replay semantic state to the given event boundary.
pub fn run_replay(repo_path: &Path, event_id: &str) -> CodexResult<Vec<SemanticUnit>> {
    let parsed = parse_uuid(event_id)?;
    let log = EventLog::new(event_log_path(repo_path));
    log.replay_to(parsed)
}

fn parse_uuid(value: &str) -> CodexResult<Uuid> {
    Uuid::parse_str(value)
        .map_err(|_| CodexError::Coordination(format!("invalid intent id: {value}")))
}

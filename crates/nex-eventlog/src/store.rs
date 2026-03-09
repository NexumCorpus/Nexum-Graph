use crate::event::SemanticEvent;
use chrono::Utc;
use nex_core::{CodexError, CodexResult, SemanticId, SemanticUnit};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// A rollback conflict caused by a later event touching the same unit.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RollbackConflict {
    pub unit: SemanticId,
    pub blocking_event: Uuid,
    pub reason: String,
}

/// Result of attempting to generate and append a rollback event.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RollbackOutcome {
    pub original_intent_id: Uuid,
    pub rollback_event: Option<SemanticEvent>,
    pub conflicts: Vec<RollbackConflict>,
}

impl RollbackOutcome {
    pub fn is_clean(&self) -> bool {
        self.conflicts.is_empty()
    }
}

/// Local-file semantic event log.
#[derive(Debug, Clone)]
pub struct EventLog {
    path: PathBuf,
}

impl EventLog {
    pub fn new(path: impl AsRef<Path>) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
        }
    }

    /// Append a semantic event to the local log file.
    pub fn append(&self, event: SemanticEvent) -> CodexResult<()> {
        let mut events = self.load_events()?;
        events.push(event);
        self.save_events(&events)
    }

    /// List all events ordered by timestamp.
    pub fn list(&self) -> CodexResult<Vec<SemanticEvent>> {
        let mut events = self.load_events()?;
        events.sort_by_key(|event| event.timestamp);
        Ok(events)
    }

    /// All events emitted for a specific intent, ordered by timestamp.
    pub fn events_for_intent(&self, intent_id: Uuid) -> CodexResult<Vec<SemanticEvent>> {
        let mut events: Vec<_> = self
            .list()?
            .into_iter()
            .filter(|event| event.intent_id == intent_id)
            .collect();
        events.sort_by_key(|event| event.timestamp);
        Ok(events)
    }

    /// Generate and append a rollback event if no later event touched the same units.
    pub fn rollback(
        &self,
        intent_id: Uuid,
        agent_id: &str,
        description: &str,
    ) -> CodexResult<RollbackOutcome> {
        let events = self.list()?;
        let target_events = self.events_for_intent(intent_id)?;
        if target_events.is_empty() {
            return Err(CodexError::Coordination(format!(
                "unknown intent: {intent_id}"
            )));
        }

        let touched_units = touched_units(&target_events);
        let last_target = target_events
            .last()
            .expect("checked non-empty target events");

        let conflicts: Vec<RollbackConflict> = events
            .iter()
            .filter(|event| event.timestamp > last_target.timestamp)
            .flat_map(|event| {
                event
                    .touched_units()
                    .into_iter()
                    .filter(|unit| touched_units.contains(unit))
                    .map(|unit| RollbackConflict {
                        unit,
                        blocking_event: event.id,
                        reason: format!(
                            "later event `{}` also touched rollback target",
                            event.description
                        ),
                    })
            })
            .collect();

        if !conflicts.is_empty() {
            return Ok(RollbackOutcome {
                original_intent_id: intent_id,
                rollback_event: None,
                conflicts,
            });
        }

        let mut compensating_mutations = Vec::new();
        for event in target_events.iter().rev() {
            for mutation in event.mutations.iter().rev() {
                compensating_mutations.push(mutation.compensate());
            }
        }

        let rollback_event = SemanticEvent {
            id: Uuid::new_v4(),
            timestamp: Utc::now(),
            intent_id: Uuid::new_v4(),
            agent_id: agent_id.to_string(),
            description: description.to_string(),
            mutations: compensating_mutations,
            parent_event: Some(last_target.id),
            tags: vec![format!("rollback:{intent_id}")],
        };

        self.append(rollback_event.clone())?;

        Ok(RollbackOutcome {
            original_intent_id: intent_id,
            rollback_event: Some(rollback_event),
            conflicts: Vec::new(),
        })
    }

    /// Rebuild semantic-unit state at a historical event boundary.
    pub fn replay_to(&self, event_id: Uuid) -> CodexResult<Vec<SemanticUnit>> {
        let events = self.list()?;
        let mut units = HashMap::new();
        let mut found = false;

        for event in events {
            for mutation in event.mutations {
                mutation.apply(&mut units);
            }
            if event.id == event_id {
                found = true;
                break;
            }
        }

        if !found {
            return Err(CodexError::Coordination(format!(
                "unknown event: {event_id}"
            )));
        }

        let mut ordered: Vec<_> = units.into_values().collect();
        ordered.sort_by(|left, right| left.qualified_name.cmp(&right.qualified_name));
        Ok(ordered)
    }

    fn load_events(&self) -> CodexResult<Vec<SemanticEvent>> {
        if !self.path.exists() {
            return Ok(Vec::new());
        }

        let content = std::fs::read_to_string(&self.path)?;
        Ok(serde_json::from_str(&content)?)
    }

    fn save_events(&self, events: &[SemanticEvent]) -> CodexResult<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = serde_json::to_string_pretty(events)?;
        std::fs::write(&self.path, content)?;
        Ok(())
    }
}

fn touched_units(events: &[SemanticEvent]) -> Vec<SemanticId> {
    let mut touched = Vec::new();
    for event in events {
        for unit in event.touched_units() {
            if !touched.contains(&unit) {
                touched.push(unit);
            }
        }
    }
    touched
}

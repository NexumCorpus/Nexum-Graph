use nex_coord::CoordinationEngine;
use nex_core::{
    AgentId, DepKind, Intent, IntentKind, SemanticId, SemanticLock, SemanticUnit, UnitKind,
};
use nex_graph::CodeGraph;
use proptest::prelude::*;
use std::collections::HashSet;
use std::path::PathBuf;

const AGENT_COUNT: u8 = 4;
const TARGET_COUNT: u8 = 4;

#[derive(Clone, Debug)]
enum Operation {
    Request {
        agent_seed: u8,
        target_index: u8,
        kind: IntentKind,
    },
    Release {
        agent_seed: u8,
        target_index: u8,
    },
    ReleaseAll {
        agent_seed: u8,
    },
}

fn make_agent(seed: u8) -> AgentId {
    [seed; 16]
}

fn make_function(name: &str, file: &str) -> SemanticUnit {
    let id_input = format!("{name}:{file}");
    let id = *blake3::hash(id_input.as_bytes()).as_bytes();
    SemanticUnit {
        id,
        kind: UnitKind::Function,
        name: name.to_string(),
        qualified_name: name.to_string(),
        file_path: PathBuf::from(file),
        byte_range: 0..100,
        signature_hash: 0,
        body_hash: 0,
        dependencies: vec![],
    }
}

fn engine_fixture() -> (
    CoordinationEngine,
    Vec<SemanticUnit>,
    HashSet<(SemanticId, SemanticId)>,
) {
    let request = make_function("process_request", "handler.ts");
    let validate = make_function("validate", "auth.ts");
    let sanitize = make_function("sanitize", "sanitize.ts");
    let format = make_function("format_date", "utils.ts");

    let mut graph = CodeGraph::new();
    for unit in [&request, &validate, &sanitize, &format] {
        graph.add_unit(unit.clone());
    }
    graph.add_dep(request.id, validate.id, DepKind::Calls);
    graph.add_dep(validate.id, sanitize.id, DepKind::Calls);

    let relations = HashSet::from([
        (request.id, validate.id),
        (validate.id, request.id),
        (validate.id, sanitize.id),
        (sanitize.id, validate.id),
    ]);

    (
        CoordinationEngine::new(graph),
        vec![request, validate, sanitize, format],
        relations,
    )
}

fn intent_kind_rank(kind: IntentKind) -> u8 {
    match kind {
        IntentKind::Read => 0,
        IntentKind::Write => 1,
        IntentKind::Delete => 2,
    }
}

fn normalize_locks(
    locks: impl IntoIterator<Item = SemanticLock>,
) -> Vec<(SemanticId, AgentId, u8)> {
    let mut normalized: Vec<_> = locks
        .into_iter()
        .map(|lock| (lock.target, lock.agent_id, intent_kind_rank(lock.kind)))
        .collect();
    normalized.sort_unstable();
    normalized
}

fn snapshot(engine: &CoordinationEngine) -> Vec<(SemanticId, AgentId, u8)> {
    normalize_locks(engine.export_locks())
}

fn without_agent(
    locks: &[(SemanticId, AgentId, u8)],
    agent_id: AgentId,
) -> Vec<(SemanticId, AgentId, u8)> {
    locks
        .iter()
        .copied()
        .filter(|(_, agent, _)| *agent != agent_id)
        .collect()
}

fn without_agent_target(
    locks: &[(SemanticId, AgentId, u8)],
    agent_id: AgentId,
    target: SemanticId,
) -> Vec<(SemanticId, AgentId, u8)> {
    locks
        .iter()
        .copied()
        .filter(|(lock_target, agent, _)| !(*agent == agent_id && *lock_target == target))
        .collect()
}

fn with_lock(
    locks: &[(SemanticId, AgentId, u8)],
    target: SemanticId,
    agent_id: AgentId,
    kind: IntentKind,
) -> Vec<(SemanticId, AgentId, u8)> {
    let mut next = locks.to_vec();
    next.push((target, agent_id, intent_kind_rank(kind)));
    next.sort_unstable();
    next
}

fn assert_query_consistency(
    engine: &CoordinationEngine,
    units: &[SemanticUnit],
    agents: &[AgentId],
) {
    let exported = snapshot(engine);
    let active = normalize_locks(engine.active_locks().into_iter().cloned());
    assert_eq!(active, exported, "active_locks diverged from export_locks");

    let state = engine.state();
    let state_locks = normalize_locks(state.locks.clone());
    assert_eq!(state_locks, exported, "state() diverged from export_locks");

    let expected_agents: HashSet<_> = exported.iter().map(|(_, agent, _)| *agent).collect();
    assert_eq!(
        state.agent_count,
        expected_agents.len(),
        "state.agent_count diverged from active lock holders"
    );

    let mut per_unit = Vec::new();
    for unit in units {
        let actual = normalize_locks(engine.locks_for_unit(&unit.id).into_iter().cloned());
        let expected: Vec<_> = exported
            .iter()
            .copied()
            .filter(|(target, _, _)| *target == unit.id)
            .collect();
        assert_eq!(
            actual, expected,
            "locks_for_unit returned a different lock set for {}",
            unit.name
        );
        per_unit.extend(actual);
    }
    per_unit.sort_unstable();
    assert_eq!(
        per_unit, exported,
        "locks_for_unit views do not partition export_locks"
    );

    let mut per_agent = Vec::new();
    for agent in agents {
        let actual = normalize_locks(engine.locks_for_agent(agent).into_iter().cloned());
        let expected: Vec<_> = exported
            .iter()
            .copied()
            .filter(|(_, holder, _)| *holder == *agent)
            .collect();
        assert_eq!(actual, expected, "locks_for_agent diverged for {:?}", agent);
        per_agent.extend(actual);
    }
    per_agent.sort_unstable();
    assert_eq!(
        per_agent, exported,
        "locks_for_agent views do not partition export_locks"
    );
}

fn assert_granted_state_invariants(
    engine: &CoordinationEngine,
    units: &[SemanticUnit],
    relations: &HashSet<(SemanticId, SemanticId)>,
    agents: &[AgentId],
) {
    assert_query_consistency(engine, units, agents);

    let locks = engine.export_locks();
    for unit in units {
        let unit_locks: Vec<_> = locks
            .iter()
            .filter(|lock| lock.target == unit.id)
            .cloned()
            .collect();

        let distinct_agents: HashSet<_> = unit_locks.iter().map(|lock| lock.agent_id).collect();
        assert_eq!(
            distinct_agents.len(),
            unit_locks.len(),
            "duplicate agent lock found on target {}",
            unit.name
        );

        if unit_locks.len() > 1 {
            assert!(
                unit_locks.iter().all(|lock| lock.kind == IntentKind::Read),
                "target {} has an illegal shared non-read lock set",
                unit.name
            );
        }
    }

    for (index, left) in locks.iter().enumerate() {
        for right in locks.iter().skip(index + 1) {
            if left.agent_id == right.agent_id {
                continue;
            }

            if matches!(left.kind, IntentKind::Write | IntentKind::Delete)
                && matches!(right.kind, IntentKind::Write | IntentKind::Delete)
            {
                assert!(
                    left.target != right.target,
                    "different agents hold conflicting write/delete locks on the same target"
                );
                assert!(
                    !relations.contains(&(left.target, right.target)),
                    "different agents hold conflicting write/delete locks on related targets"
                );
            }
        }
    }
}

fn arbitrary_intent_kind() -> impl Strategy<Value = IntentKind> {
    prop_oneof![
        Just(IntentKind::Read),
        Just(IntentKind::Write),
        Just(IntentKind::Delete),
    ]
}

fn arbitrary_operation() -> impl Strategy<Value = Operation> {
    prop_oneof![
        (0u8..AGENT_COUNT, 0u8..TARGET_COUNT, arbitrary_intent_kind()).prop_map(
            |(agent_seed, target_index, kind)| Operation::Request {
                agent_seed,
                target_index,
                kind,
            },
        ),
        (0u8..AGENT_COUNT, 0u8..TARGET_COUNT).prop_map(|(agent_seed, target_index)| {
            Operation::Release {
                agent_seed,
                target_index,
            }
        }),
        (0u8..AGENT_COUNT).prop_map(|agent_seed| Operation::ReleaseAll { agent_seed }),
    ]
}

proptest! {
    #[test]
    fn coordination_engine_preserves_lock_invariants(
        operations in proptest::collection::vec(arbitrary_operation(), 1..160)
    ) {
        let (mut engine, units, relations) = engine_fixture();
        let agents: Vec<_> = (0..AGENT_COUNT).map(make_agent).collect();

        for operation in operations {
            match operation {
                Operation::Request { agent_seed, target_index, kind } => {
                    let agent_id = make_agent(agent_seed);
                    let target = units[target_index as usize].id;
                    let before = snapshot(&engine);
                    let result = engine.request_lock(Intent { agent_id, target, kind });
                    let after = snapshot(&engine);

                    if matches!(result, nex_core::LockResult::Granted) {
                        assert_eq!(
                            after,
                            with_lock(&before, target, agent_id, kind),
                            "granted request did not add exactly one new lock"
                        );
                    } else {
                        assert_eq!(after, before, "denied request mutated lock state");
                    }
                }
                Operation::Release { agent_seed, target_index } => {
                    let agent_id = make_agent(agent_seed);
                    let target = units[target_index as usize].id;
                    let before = snapshot(&engine);
                    let result = engine.release_lock(&agent_id, &target);
                    let after = snapshot(&engine);

                    if result.is_ok() {
                        assert_eq!(
                            after,
                            without_agent_target(&before, agent_id, target),
                            "successful release_lock removed the wrong lock set"
                        );
                    } else {
                        assert_eq!(after, before, "failed release_lock mutated state");
                    }
                }
                Operation::ReleaseAll { agent_seed } => {
                    let agent_id = make_agent(agent_seed);
                    let before = snapshot(&engine);
                    engine.release_all(&agent_id);
                    let after = snapshot(&engine);

                    assert_eq!(
                        after,
                        without_agent(&before, agent_id),
                        "release_all did not remove exactly the target agent's locks"
                    );
                }
            }

            assert_granted_state_invariants(&engine, &units, &relations, &agents);
        }
    }
}

# Core Invariants

The `nex-graph` and `nex-coord` layers carry infrastructure-grade correctness requirements. This file freezes the behavioral contracts that hardening tests are expected to enforce.

## Parse / Identity Invariants

- `SemanticUnit.id` is content-addressed as `BLAKE3("{qualified_name}:{file_path}:{body_hash}")`.
- Re-extracting the same source at the same path must produce the same `SemanticUnit.id`.
- A body-only change must change `SemanticUnit.id`.
- A signature-only change with an unchanged normalized body must preserve `SemanticUnit.id`.
- Extractor dependency edges must only reference units emitted by the same extraction pass.
- `dependencies(units, source)` receives every unit of the snapshot and must resolve which file `source` is from its content, never from the position of units in the slice: a file's edges must not depend on whether it is walked first. Byte-identical files each receive their edges; a source whose units are absent from `units` yields no edges.

## Graph Diff Invariants

- `CodeGraph::diff()` matches units by `qualified_name`, not insertion order or `SemanticId`.
- A qualified name may appear in at most one diff bucket: `added`, `removed`, `modified`, or `moved`.
- A unit is `moved` only when `signature_hash` and `body_hash` are unchanged and `file_path` changes.
- A unit is `modified` when either `signature_hash` or `body_hash` changes, even if `file_path` also changes.
- Dependency edges do not affect diff classification; they affect coordination and validation later, not unit identity.
- `CodeGraph::add_dep()` is idempotent per `(from, to, kind)`: re-adding an existing edge does not create a parallel duplicate.
- Rebuilding the same graph content in a different insertion order must not change diff output.

## Conflict Detection Invariants

- `DeletedDependency` and `SignatureMismatch` count a caller in the dependent branch only if that branch's copy of the caller survives a 3-way merge. A caller the dependent branch left identical to base (same file, signature and body) while the changing branch modified or removed it is superseded by the changing branch's version and is not reported.
- Callers the dependent branch added or modified are always reported, as are callers neither branch touched.
- A name-matched `SignatureChanged` entry is not reported as a `SignatureMismatch` when the changing branch still holds the base unit with unchanged signature and body at its original file; the entry is a same-named unit added elsewhere, not a change to that unit.

## Coordination / CRDT Invariants

- Denied lock requests must not mutate engine state.
- Failed `release_lock()` calls must not mutate engine state.
- Granted `request_lock()` calls add exactly one new `SemanticLock`.
- Successful `release_lock()` removes exactly one `(agent_id, target)` lock and preserves every other lock.
- `release_all(agent)` removes all and only that agent's locks.
- A target may hold multiple concurrent locks only when every active lock on that target is `Read` and each lock belongs to a different agent.
- No two different agents may hold `Write`/`Delete` locks on directly related units at the same time.
- Query surfaces (`active_locks`, `locks_for_unit`, `locks_for_agent`, `export_locks`, `state`) must describe the same active lock set.
- Distributed reconciliation must never leave incompatible active intents alive simultaneously after replica merge.
- Replica rebuild is deterministic: given the same active intent set, every replica must keep the same winning intents.
- Deterministic winner selection is ordered by `intent_id` during CRDT rebuild. Lower `intent_id` wins when incompatible intents cannot coexist.
- Disjoint compatible intents must converge as a union across replicas.
- Once a losing intent is removed during reconciliation, replaying stale pre-reconciliation updates must not resurrect it.
- Commit, abort, and expiry removals must propagate through the CRDT document so other replicas release the same locks.

## Persistence Invariants

- `.nex` state files are written atomically with backup recovery.
- Corrupt primary coordination state must recover from a valid sibling backup before failing hard.
- If `.nex/coordination.loro` is unreadable but `.nex/locks.json` remains valid, CLI lock workflows fall back to the JSON snapshot instead of failing closed.
- Rewriting lock state from a known-good snapshot must be able to replace a corrupt CRDT file.
- Event-log append must preserve prior history when the primary file is missing but a valid backup exists.

## Change Discipline

If a change touches `nex-parse` unit identity or dependency behavior, `nex-graph` diff behavior, `nex-coord` conflict detection, or `nex-coord` distributed reconciliation:

1. Update this file if the intended invariant changes.
2. Add or update tests that encode the new invariant directly.
3. Treat a behavior change in these layers as a contract change, not a refactor.

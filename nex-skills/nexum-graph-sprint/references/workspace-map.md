# Workspace Map

## Crates

- `nex-core`: shared semantic types and diff models
- `nex-parse`: language extractors and extractor registry
- `nex-graph`: semantic graph construction and diffing
- `nex-coord`: conflict detection, lock engine, HTTP coordination service
- `nex-validate`: pre-commit validation over semantic diffs and locks
- `nex-eventlog`: append-only event store, replay, rollback
- `nex-lsp`: Codex-aware LSP shim and upstream proxy layer
- `nex-cli`: `nex` command surface and end-to-end pipelines

## Naming Transition

- Public project name: `Nexum Graph`
- Current workspace crates: `nex-*`
- Current local state directory: `.nex/`
- Legacy names may still appear in older prompts or conversation as `Project Codex`

## Repo Tools

- Spec search:
  `python tools/spec_query.py locking --doc spec`
- Phrase search with cache/source stats:
  `python tools/spec_query.py "CRDT coordination" --mode phrase --stats`
- Targeted verification by crate:
  `python tools/verify_slice.py --crate nex-coord`
- Verification inferred from changed files:
  `python tools/verify_slice.py --changed`
- Verification relative to another ref:
  `python tools/verify_slice.py --since origin/master`
- Fast inspection without execution:
  `python tools/verify_slice.py --crate nex-lsp --dry-run`
- Workspace health and rename scan:
  `python tools/workspace_doctor.py --legacy-scan`
- Skill install or drift check:
  `python tools/sync_nex_skills.py`
  `python tools/sync_nex_skills.py --check`

## Default Slice Pattern

1. Search the implementation spec or whitepaper for the feature.
2. Inspect the owning crate and its nearest tests.
3. Implement the smallest coherent vertical slice.
4. Add or extend tests in the owning crate.
5. Run `tools/verify_slice.py` for the impacted crates.
6. Report passed commands and the next remaining gap.

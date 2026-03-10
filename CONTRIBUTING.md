# Contributing to Nexum Graph

Thanks for taking the repo seriously enough to improve it.

## Ground rules

- Keep changes scoped and explainable.
- Preserve deterministic behavior across parse, graph, coordination, validation, and logging layers.
- Treat Clippy, formatting, and tests as release gates, not suggestions.
- Prefer extending existing crate boundaries over inventing new ones without a strong reason.

## Local setup

```bash
git clone https://github.com/NexumCorpus/Nexum-Graph.git
cd Nexum-Graph
cargo build --release
python -m unittest discover -s tools -p "test_*.py"
```

Recommended tooling:

- Rust stable with edition 2024 support
- `clippy`
- `rustfmt`
- Python 3.10+
- Node 20+
- Git

Optional but recommended for local merge safety:

```bash
nex check --install-hook
```

## Repository layout

- `crates/nex-core`
  Shared contracts and persistence helpers.
- `crates/nex-parse`
  Language extractors.
- `crates/nex-graph`
  Graph and diff logic.
- `crates/nex-coord`
  Coordination engine, service layer, CRDT state.
- `crates/nex-validate`
  Validation rules.
- `crates/nex-eventlog`
  Event storage and replay.
- `crates/nex-lsp`
  LSP integration.
- `crates/nex-cli`
  Operator-facing CLI, server, auth, and audit workflows.
- `tools/`
  Python helper and verification scripts.

## Validation workflow

For targeted work, use:

```bash
python tools/verify_slice.py --changed
```

For a full sweep, use:

```bash
python tools/release_tools.py assert-version-parity --tag v0.1.0
python tools/project_facts.py check-readme
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --check
python -m unittest discover -s tools -p "test_*.py"
npm --prefix extensions/vscode test
```

If you change Python tools or Codex skills, also run:

```bash
python tools/tool_selftest.py
```

## Documentation expectations

If your change affects how someone evaluates, installs, secures, or operates Nexum Graph, update the public docs in the same pull request.

That usually means one or more of:

- `README.md`
- `CONTRIBUTING.md`
- `CORE_INVARIANTS.md`
- `RELEASING.md`
- `SECURITY.md`
- command help text

## Security-sensitive changes

If your work touches:

- server auth
- audit logging
- remote binding behavior
- persistence or recovery logic
- rollback or replay integrity

include tests that prove both the happy path and the failure path.

## Pull requests

Good pull requests generally include:

- a tight summary of what changed
- why the change belongs in this crate or layer
- verification commands that were run
- any residual risk or follow-up work

If the change is user-visible, include the operator-facing usage path, not just the implementation detail.

If the change affects packaging, installers, workflows, or artifact naming, update [RELEASING.md](./RELEASING.md) in the same pull request.

If the change affects `nex check`, git hook behavior, or the published GitHub Action in [action.yml](./action.yml), update [README.md](./README.md) with the operator-facing usage path in the same pull request.

If the change affects public-facing counts or repo facts, run `python tools/project_facts.py sync-readme` and include the regenerated facts block from [README.md](./README.md).

If the change affects semantic unit identity, graph diff semantics, or distributed coordination behavior, update [CORE_INVARIANTS.md](./CORE_INVARIANTS.md) in the same pull request.

## Fuzzing Core Contracts

The parser and graph layers now have dedicated libFuzzer targets in [fuzz](./fuzz). Use them when you touch semantic extraction, graph construction, or diff classification:

```bash
cargo fuzz run semantic_pipeline
cargo fuzz run graph_diff
```

If you only need a compile smoke check for the fuzz package:

```bash
cargo check --manifest-path fuzz/Cargo.toml --bins
```

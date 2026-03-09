# Nexum Graph

AI-native code coordination for multi-agent software engineering.

Nexum Graph turns a codebase into a semantic graph, coordinates multiple agents against that graph, validates whether changes respect declared intent, records those changes in an event log, and exposes the system over both CLI and LSP surfaces.

This repository is the full product codebase:

- 8 Rust crates in one workspace
- 12 CLI commands
- TypeScript, Python, and Rust semantic extraction
- Semantic diff, coordination, validation, event log, HTTP server, and LSP integration
- 230+ automated tests across Rust and Python

## Why This Exists

Most AI coding tools still coordinate at the file or patch level. That breaks down when multiple agents touch the same repo at once.

Nexum Graph coordinates at the semantic unit level instead:

- Functions, methods, classes, traits, and other code units are first-class entities.
- Dependencies between those entities are explicit.
- Agents declare intent before writing.
- Validation checks whether the resulting changes were actually authorized.
- The system leaves behind an auditable trail instead of a pile of opaque patches.

The result is a foundation for multi-agent coding systems that need stronger guarantees than "hope the diffs merge."

## What You Can Do Today

- Compute semantic diffs between refs with `nex diff`.
- Detect merge-risk conflicts between branches with `nex check`.
- Acquire semantic locks on specific units with `nex lock`.
- Validate lock coverage before commit with `nex validate`.
- Run a local coordination server with `nex serve`.
- Bootstrap secure remote access with `nex auth`.
- Verify the tamper-evident audit trail with `nex audit verify`.
- Stream coordination signals into editors through `nex-lsp`.

## Quickstart

### 1. Clone and build

```bash
git clone https://github.com/NexumCorpus/Nexum-Graph.git
cd Nexum-Graph
cargo build --release
```

If you want the helper tools as well:

```bash
python -m unittest discover -s tools -p "test_*.py"
```

### 2. Get value in under five minutes

Run a semantic diff:

```bash
cargo run -p nex-cli -- diff HEAD~1 HEAD
```

Bootstrap secure server auth:

```bash
cargo run -p nex-cli -- auth init --agent alice --agent bob
```

Start the coordination server:

```bash
cargo run -p nex-cli -- serve --host 127.0.0.1 --port 4000
```

Verify the audit trail:

```bash
cargo run -p nex-cli -- audit verify
```

Once built, you can replace `cargo run -p nex-cli --` with the installed `nex` binary.

## Product Surface

### CLI commands

| Command | Purpose |
|---|---|
| `nex diff` | Semantic diff between two git refs |
| `nex check` | Conflict detection between two branches |
| `nex lock` | Acquire a semantic lock |
| `nex unlock` | Release a semantic lock |
| `nex locks` | List active locks |
| `nex validate` | Validate lock coverage against a base ref |
| `nex log` | Show semantic event history |
| `nex rollback` | Generate a rollback event |
| `nex replay` | Replay semantic state to an event boundary |
| `nex auth` | Bootstrap and manage server auth |
| `nex audit verify` | Verify the tamper-evident audit trail |
| `nex serve` | Start the coordination server |

### LSP surface

The `nex-lsp` binary provides editor integration with:

- `nex/semanticDiff`
- `nex/activeLocks`
- `nex/agentIntent`
- `nex/validationStatus`
- `nex/eventStream`

It also proxies standard LSP requests to an upstream server and merges Nexum Graph overlays into the editor experience.

## Architecture

Nexum Graph is built as a five-layer chassis:

1. Semantic Code Graph
   Parses source into semantic units and dependency edges.
2. Intent Coordination
   Declares intent, grants locks, detects conflicts, and converges distributed state with CRDTs.
3. Continuous Validation
   Checks whether modifications and deletions are covered by locks and flags broken references.
4. Immutable Event Log
   Records semantic mutations for replay and rollback.
5. IDE and Server Integration
   Exposes the system through CLI, HTTP, WebSocket, and LSP surfaces.

### Workspace map

| Crate | Purpose |
|---|---|
| `nex-core` | Shared contracts, types, errors, persistence helpers |
| `nex-parse` | Tree-sitter extraction and language-specific semantic extractors |
| `nex-graph` | Semantic graph construction and graph diffing |
| `nex-coord` | Coordination engine, conflict detection, service layer, CRDT state |
| `nex-validate` | Lock coverage and semantic validation rules |
| `nex-eventlog` | Event storage, replay, rollback, backend abstraction |
| `nex-lsp` | Editor integration and upstream LSP proxy |
| `nex-cli` | CLI, server, auth, audit, and operator workflows |

## Supported Languages

| Language | Current extractor coverage |
|---|---|
| TypeScript / TSX | Functions, classes, methods, interfaces, enums, type aliases |
| Python | Functions, classes, methods, decorators, async variants |
| Rust | Functions, structs, enums, traits, impl methods, inline modules |

The extractor trait is intentionally extensible, so more languages can be added without changing the rest of the chassis.

## Core Workflows

### Semantic diff

```bash
nex diff main feature/refactor --format text
nex diff v0.1.0 HEAD --format json
```

### Conflict detection

```bash
nex check feature/a feature/b
```

### Locking and validation

```bash
nex lock alice validate write
nex validate alice --base HEAD~1
nex unlock alice validate
```

### Secure server bootstrap

```bash
nex auth init --agent alice --agent bob
nex serve --host 0.0.0.0 --port 4000
nex audit verify
```

## Security and Operations

Nexum Graph now includes operator-oriented hardening out of the box:

- Remote binds are rejected unless auth is configured or explicitly bypassed.
- Repo-local auth config stores BLAKE3 token hashes at rest, not raw bearer secrets.
- Raw auth tokens are only shown once at issue time.
- Server audit records are hash chained and anchored by `.nex/server-audit.head.json`.
- `nex audit verify` detects record edits, missing anchors, and tail truncation.
- Existing plaintext auth configs are still readable for migration compatibility.

State lives under `.nex/`:

| File | Purpose |
|---|---|
| `coordination.loro` | CRDT coordination state |
| `locks.json` | Compatibility snapshot of active locks |
| `events.json` | Local event log when using the file backend |
| `server-auth.json` | Reloadable auth config with token hashes at rest |
| `server-audit.jsonl` | Hash-chained server audit trail |
| `server-audit.head.json` | Audit head anchor for integrity verification |
| `cache/` | Local tool caches |

For repo security policy and reporting guidance, see [SECURITY.md](./SECURITY.md).

## Developer Workflow

The repo includes a small toolchain for spec-driven, slice-based development:

- `tools/spec_query.py`
  Searches the `.docx` implementation spec and whitepaper files.
- `tools/verify_slice.py`
  Runs tests, Clippy, and format checks only for changed crates and downstream dependents.
- `tools/workspace_doctor.py`
  Checks repo health, tool availability, and skill sync state.
- `tools/sync_codex_skills.py`
  Installs repo-managed Codex skills into `$CODEX_HOME`.
- `tools/tool_selftest.py`
  Runs the Python tool regression suite and skill checks.

Typical workflow:

```bash
python tools/verify_slice.py --changed
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --check
```

If you plan to contribute, start with [CONTRIBUTING.md](./CONTRIBUTING.md).

## Project Status

Implemented and usable today:

- Semantic extraction for TypeScript, Python, and Rust
- Graph-based diffing
- Coordination engine and CRDT-backed lock state
- Validation engine
- Event log with replay and rollback
- Local coordination server
- Auth bootstrap, rotation, revocation, and audit verification
- LSP shim with upstream proxy support

Active expansion areas:

- More language extractors
- Stronger remote trust anchoring for audit provenance
- Broader editor packaging and distribution
- Distributed backend and deployment ergonomics

## Philosophy

Nexum Graph is opinionated:

- Semantic units matter more than raw files.
- Deterministic contracts matter more than agent improvisation.
- Compiler and validator feedback should be part of the agent loop.
- Operational safety has to be part of the product, not an afterthought.

This repo is also a live example of spec-driven multi-agent Rust development. The codebase itself is part of the thesis.

## Contributing

Please read [CONTRIBUTING.md](./CONTRIBUTING.md) before opening a pull request.

## License

This project is licensed under the MIT License. See [LICENSE](./LICENSE).

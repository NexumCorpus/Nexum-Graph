# Project Codex

**AI-native code coordination for multi-agent software engineering.**

Project Codex is a deterministic coordination layer that enables multiple AI coding agents to work on the same codebase without corrupting each other's changes. It parses code into a semantic graph, provides intent-based locking, validates changes against lock ownership, and maintains an immutable event log for rollback and replay.

Zero lines of Rust were written by a human. Claude wrote the types, tests, and prompts. OpenAI Codex filled in every function body.

## Architecture

Five-layer deterministic chassis:

```
 Layer 1 ─ Semantic Code Graph    tree-sitter CST → petgraph DiGraph
 Layer 2 ─ Intent Coordination    semantic locking, conflict detection
 Layer 3 ─ Continuous Validation  pre-commit lock coverage checks
 Layer 4 ─ Immutable Event Log    append-only log, rollback, replay
 Layer 5 ─ IDE Integration        LSP shim + HTTP coordination server
```

## Workspace

8 crates in a Cargo workspace:

| Crate | Purpose |
|---|---|
| `codex-core` | Authoritative types (`SemanticUnit`, `SemanticId`, `SemanticDiff`, `DepKind`) |
| `codex-parse` | Tree-sitter extraction, `SemanticExtractor` trait, TypeScript extractor |
| `codex-graph` | `petgraph` semantic graph, diff algorithm |
| `codex-coord` | Conflict detection, coordination engine, intent lifecycle |
| `codex-validate` | Pre-commit validation (lock coverage, broken references, stale callers) |
| `codex-eventlog` | Append-only event log, compensating rollback, state replay |
| `codex-lsp` | `tower-lsp` proxy with semantic diff, lock annotations, event streaming |
| `codex-cli` | CLI binary with 10 subcommands |

## Installation

### Prerequisites

- Rust 1.85+ (edition 2024)
- Git (for repository operations)

### Build from source

```bash
git clone https://github.com/NexumCorpus/Project-Codex.git
cd Project-Codex
cargo build --release
```

The binary is at `target/release/codex` (or `target/release/codex.exe` on Windows).

### Verify

```bash
cargo test --workspace
```

All 147 tests should pass.

## Commands

### `codex diff` — Semantic diff between two git refs

```bash
codex diff v1 v2 --format text
codex diff main feature-branch --format json
```

Computes a semantic diff showing added, removed, and modified code units (functions, classes, methods) between two git refs. Understands signature vs body-only changes.

### `codex check` — Conflict detection between branches

```bash
codex check branch-a branch-b
codex check feature-1 feature-2 --format json
```

Three-way merge analysis that detects four conflict types:
- **Concurrent Modification** — same function modified on both branches
- **Signature Drift** — function signature changed, callers may break
- **Broken Reference** — dependency target deleted or moved
- **Stale Caller** — caller body unchanged but callee signature changed

### `codex lock` — Acquire a semantic lock

```bash
codex lock alice validate write
codex lock bob processRequest read
```

Requests a semantic lock on a named code unit. Lock kinds: `read`, `write`, `delete`. Multiple read locks are compatible; write and delete locks are exclusive.

### `codex unlock` — Release a semantic lock

```bash
codex unlock alice validate
```

### `codex locks` — List active locks

```bash
codex locks
codex locks --format json
```

### `codex validate` — Check lock coverage

```bash
codex validate alice --base HEAD~1
codex validate bob --base main --format json
```

Validates that all modifications in the working tree are covered by semantic locks held by the named agent. Reports unlocked modifications, unlocked deletions, broken references, and stale callers.

### `codex log` — Event history

```bash
codex log
codex log --intent-id <uuid> --format json
```

Shows the semantic event log (`.codex/events.json`). Each event records an agent's committed intent with mutations, parent event, and tags.

### `codex rollback` — Semantic rollback

```bash
codex rollback <intent-id> alice
```

Generates compensating mutations that reverse a prior intent's changes. Detects conflicts if later events modified the same units.

### `codex replay` — Replay state to a point in time

```bash
codex replay --to <event-id>
```

Applies all mutations in order up to the specified event, producing the semantic state at that point.

### `codex serve` — Coordination server

```bash
codex serve --host 127.0.0.1 --port 4000
```

Starts an HTTP + WebSocket coordination server exposing:
- `POST /intent/declare` — declare intent with automatic lock acquisition
- `POST /intent/commit` — commit intent, append to event log, release locks
- `POST /intent/abort` — abort intent, release locks
- `GET /graph/query` — query the semantic graph
- `GET /locks` — list active locks
- `GET /events` — WebSocket stream of coordination events

## LSP Server

The `codex-lsp` binary provides IDE integration via the Language Server Protocol:

```bash
codex-lsp --repo-path . --base-ref HEAD~1
```

Custom LSP methods:
- `codex/semanticDiff` — file-scoped semantic diff
- `codex/activeLocks` — lock annotations as code lenses
- `codex/validationStatus` — real-time validation diagnostics
- `codex/eventStream` — semantic event notifications

## How It Works

1. **Parse**: Tree-sitter parses source files into concrete syntax trees. The `SemanticExtractor` trait maps CST nodes to `SemanticUnit` values with content-addressed IDs (BLAKE3), signature hashes, and normalized body hashes.

2. **Graph**: Units and their dependency edges (calls, imports, inheritance) form a `petgraph` directed graph. Diffing two graphs produces added/removed/modified classifications.

3. **Coordinate**: Agents declare intents targeting specific units. The coordination engine acquires semantic locks, checks for conflicts with existing lock holders, and manages intent lifecycles (declare → commit/abort) with TTL-based expiry.

4. **Validate**: Before commit, the validation engine checks that every modification is covered by a write lock, every deletion by a delete lock, and flags broken references and stale callers.

5. **Log**: Committed intents produce immutable semantic events with structured mutations. The event log supports compensating rollback and state replay.

## Numbers

- **147** tests, all passing
- **5** architectural layers
- **10** CLI commands
- **8** workspace crates
- **0** lines of Rust written by a human

## License

MIT

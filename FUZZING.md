# Fuzzing

Nexum Graph's highest-risk contracts live at the semantic boundary:

- extractor stability on arbitrary bytes
- graph construction from extracted units and dependency edges
- diff classification invariants across arbitrary unit sets

The repo ships dedicated libFuzzer targets under [fuzz](./fuzz):

- `semantic_pipeline`
  Runs the TypeScript, Python, and Rust extractors against arbitrary bytes, builds a `CodeGraph`, and asserts extractor and graph invariants.
- `graph_diff`
  Builds arbitrary synthetic graphs and asserts that `CodeGraph::diff()` preserves the bucket and classification contracts frozen in [CORE_INVARIANTS.md](./CORE_INVARIANTS.md).

## Setup

Install `cargo-fuzz` once:

```bash
cargo install cargo-fuzz
```

## Run Targets

From the repo root:

```bash
cargo fuzz run semantic_pipeline
cargo fuzz run graph_diff
```

To start from the checked-in seeds:

```bash
cargo fuzz run semantic_pipeline fuzz/corpus/semantic_pipeline
```

## Cheap Smoke Check

If you only want to verify that the fuzz package still compiles:

```bash
cargo check --manifest-path fuzz/Cargo.toml --bins
```

## Windows Note

The fuzz package compile-checks on Windows/MSVC, but live `cargo fuzz run ...`
is not currently validated in this environment. On the machine used for the
release sweep, `cargo-fuzz` links failed with unresolved `__sancov_*` symbols
even with `--sanitizer none`. For live fuzz campaigns, prefer Linux or macOS
until the MSVC sanitizer-coverage toolchain issue is resolved.

## When To Touch These

Update the fuzz targets when you change:

- semantic unit identity rules
- extractor dependency edge logic
- graph diff classification rules

Treat crashes or invariant failures here as core regressions, not low-priority test noise.

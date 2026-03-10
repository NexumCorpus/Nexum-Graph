---
name: nexum-graph-sprint
description: Use when building, extending, or continuing Nexum Graph across semantic parsing, graph diffing, coordination, validation, event logging, LSP, CLI, or spec-driven architecture slices. Also use when the user says continue, keep going, build out, onward, implementation plan, whitepaper, or refers to the project by its previous name.
---

# Nexum Graph Sprint

Use this skill for spec-driven buildout of the Nexum Graph workspace. The project was previously named Project Codex, so older prompts or discussion may still use the old name even though the live crates and binaries now use `nex-*` names.

## Workflow

1. Establish current repo state before coding.
   Run `python tools/workspace_doctor.py` when the workspace state, local skills, or developer toolchain may be relevant. Read `README.md`, inspect `git status --short`, and look at the most recent commits if the current slice is unclear.
2. Ground the task in the implementation docs.
   Use `python tools/spec_query.py <terms>` to search the Markdown implementation spec or whitepapers instead of manually opening archival `.docx` files. Prefer `--mode phrase` for exact architectural terms and `--stats` when cache freshness matters.
3. Pick one vertical slice.
   Prefer finishing the next coherent slice end to end rather than scattering changes across unrelated layers. Keep changes aligned to the deterministic chassis: parse -> graph -> coord -> validate -> eventlog -> lsp -> cli. When the user asks for robustness, first improve the owning tool or interface instead of papering over the same friction in-line.
4. Verify with workspace-aware scope.
   Use `python tools/verify_slice.py` to run targeted `cargo test`, `cargo clippy`, and `cargo fmt --check` for the touched crate set and their transitive dependents. Use `--changed` for dirty-tree work, `--since <rev>` for branch comparisons, and `--json` when another tool needs structured output.
5. Leave the repo in a reproducible state.
   Mention which commands passed, which files changed, and any residual gap that remains for the next slice.

## Default Working Rules

- Prefer the current `nex-*` crate names, binary names, and `.nex/` state paths.
- Treat `Project Codex` as a legacy alias only when reading older docs or filenames.
- Keep the implementation spec authoritative when README text and prompts diverge.
- Add or extend tests in the crate that owns the behavior, then run the targeted verification slice.
- When a task spans multiple crates, follow dependency direction rather than editing top-level surfaces first.
- Use `python tools/workspace_doctor.py --legacy-scan` before broad rename cleanup so the hit list is reproducible instead of anecdotal.
- Treat repo-level developer tools as first-class code. When they change, smoke-test them directly and report which commands you exercised.

## References

- Read [references/workspace-map.md](references/workspace-map.md) when you need the crate map, naming transition notes, or verification entry points.
- Read [references/tooling-playbook.md](references/tooling-playbook.md) when you need to choose between the repo doctor, spec search, or slice verifier.

# Tooling Playbook

## Repo Doctor

- Use `python tools/workspace_doctor.py` at the start of a turn when repo health, local skills, missing commands, or impacted crates are unclear.
- Add `--legacy-scan` before rename cleanup or branding work.
- Use `--json` only when another script needs structured output.

## Tool Regression Harness

- Use `python tools/tool_selftest.py` after changing repo tools or local Nexum skills.
- It runs Python compilation, the `tools/test_repo_tools.py` regression suite, `workspace_doctor.py`, and local skill validation.
- Use `--skip-skills` if you only need repo-tool validation.

## Spec Search

- Use `python tools/spec_query.py <terms>` for implementation-plan grounding.
- Prefer `--mode phrase` for exact protocol names or section labels.
- Prefer `--mode any` when searching broad themes like `lock` or `rollback`.
- Add `--stats` when you need to confirm whether results came from cache or a fresh `.docx` extraction.
- Add `--refresh` only when the source docs changed.

## Slice Verification

- Use `python tools/verify_slice.py --crate <crate>` when you already know the owning crate.
- Use `python tools/verify_slice.py --changed` for dirty-tree work.
- Use `python tools/verify_slice.py --since <rev>` for branch-relative validation.
- Use `--no-dependents` only when intentionally checking a leaf crate in isolation.
- Treat `No Rust crates impacted.` as valid for tool-only, skill-only, or docs-only changes.

## Skill Sync

- Use `python tools/sync_codex_skills.py` after editing `codex-skills/` to install or refresh the local copies under `$CODEX_HOME/skills`.
- Use `python tools/sync_codex_skills.py --check` to detect local drift without writing.

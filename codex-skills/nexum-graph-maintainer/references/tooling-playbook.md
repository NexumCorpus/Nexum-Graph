# Tooling Playbook

## Core Repo Tools

- `python tools/workspace_doctor.py`
  Use first when you need a health snapshot of commands, docs, skills, dirty files, or impacted crates.
- `python tools/spec_query.py`
  Use for implementation-plan lookups instead of opening `.docx` internals directly.
- `python tools/verify_slice.py`
  Use for crate-scoped verification. Prefer `--changed` for dirty work and `--since <rev>` for branch-relative checks.
- `python tools/tool_selftest.py`
  Use after changing repo tools or local Nexum skills to run the Python regression suite and skill validation in one pass.
- `python tools/sync_codex_skills.py`
  Use to install repo-managed skill copies into `$CODEX_HOME/skills` and to detect local drift with `--check`.

## Local Skills

- `nexum-graph-sprint`
  Use for product and architecture slices across the workspace.
- `nexum-graph-maintainer`
  Use for toolsmithing, workflow hardening, skill updates, and rename hygiene.

## Validation Commands

- Repo skill validation:
  `python C:\Users\dalea\.codex\skills\.system\skill-creator\scripts\quick_validate.py codex-skills\nexum-graph-sprint`
  `python C:\Users\dalea\.codex\skills\.system\skill-creator\scripts\quick_validate.py codex-skills\nexum-graph-maintainer`
- Skill drift check:
  `python tools/sync_codex_skills.py --check`
- Dynamic verifier dry run:
  `python tools/verify_slice.py --changed --dry-run`
- Repo doctor with rename scan:
  `python tools/workspace_doctor.py --legacy-scan`
- Full repo-tool selftest:
  `python tools/tool_selftest.py`

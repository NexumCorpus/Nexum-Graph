---
name: nexum-graph-maintainer
description: Use when improving Nexum Graph developer tooling, Codex skills, verification workflows, spec search, repo hygiene, rename cleanup, or local developer experience. Also use when the user asks to make Codex more robust, improve skills and tools, strengthen workflow automation, or harden the maintenance surface for Project Codex or Nexum Graph.
---

# Nexum Graph Maintainer

Use this skill when the task is not a product feature slice but a capability slice for future Nexum Graph work.

## Workflow

1. Diagnose before editing.
   Run `python tools/workspace_doctor.py` and inspect the current repo tool surface, local skills, and dirty state.
2. Ground improvements in the actual repo.
   Read the current tool or skill files before changing them. If the work is tied to the implementation docs, search them with `python tools/spec_query.py`.
3. Fix one leverage point at a time.
   Prefer changes that remove repeat friction for future sessions: better verifier logic, better repo diagnostics, better skill instructions, or better rename hygiene.
4. Validate the new maintenance surface directly.
   Smoke-test each touched Python tool with concrete commands. Run `python tools/verify_slice.py --changed --dry-run` after repo-tool changes to confirm the verifier still resolves the workspace.
5. Validate local skills.
   Run `python tools/sync_codex_skills.py --check` after changing `codex-skills/`, and install updates with `python tools/sync_codex_skills.py` when the local copies are behind.

## Working Rules

- Keep skills concise and procedural; move detail into references instead of bloating `SKILL.md`.
- Prefer dynamic workspace discovery over hard-coded crate maps when tool behavior can be derived from Cargo metadata.
- Treat `Project Codex`, `codex-*`, and `.codex/` as legacy identifiers that may still appear in docs, comments, or compatibility paths.
- Leave compatibility shims in place when they reduce migration risk, but make the preferred path obvious.
- Report both tool smoke tests and skill validation commands in the final handoff.

## References

- Read [references/tooling-playbook.md](references/tooling-playbook.md) for the current repo tools and when to use them.
- Read [references/maintenance-checklist.md](references/maintenance-checklist.md) when doing repo hygiene, rename cleanup, or skill updates.

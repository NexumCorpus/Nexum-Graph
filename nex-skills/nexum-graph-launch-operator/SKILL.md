---
name: nexum-graph-launch-operator
description: Use when preparing, releasing, monitoring, or validating Nexum Graph's public GitHub surface. This includes GitHub Releases, release workflow monitoring, public repo state checks, README claim alignment, distribution trust fixes, launch-day triage, and post-release verification.
---

# Nexum Graph Launch Operator

Use this skill when the task is about Nexum Graph as a public product surface rather than an internal feature slice.

## Workflow

1. Snapshot the public state first.
   Run `python tools/github_release_watch.py public-summary` to see the visible repo state before changing docs, release claims, or launch messaging.
2. For release work, monitor the tag directly.
   Use `python tools/github_release_watch.py release-status --tag vX.Y.Z` for a one-shot check, or add `--wait-seconds` when you need to watch a release workflow through to completion.
3. Align claims with reality.
   Treat "tag exists", "workflow succeeded", and "GitHub Release object with assets exists" as three separate states. Do not claim the release is live until all three are true.
4. Fix the smallest credibility gap first.
   Prefer changes that remove public trust breaks: broken workflows, stale README claims, missing assets, or docs that point to non-existent release surfaces.
5. Re-check the public surface after push or retag.
   Confirm the release object, asset list, and public README/install story with the repo tool instead of relying on browser refreshes alone.

## Working Rules

- A tag is not a release. A green local build is not a public asset. Verify both.
- Treat GitHub Actions workflow names, asset names, and installer contracts as public API surfaces.
- If a release workflow is failing on a specific runner, fix the workflow matrix before moving the tag again.
- Keep the public repo honest. If a release is not published, README copy should not imply that it already is.
- When you change `nex-skills/`, run `python tools/sync_nex_skills.py` so the installed local copy stays current.

## References

- Read [references/public-state-playbook.md](references/public-state-playbook.md) for the public release/trust checklist.
- Read [../nexum-graph-maintainer/references/tooling-playbook.md](../nexum-graph-maintainer/references/tooling-playbook.md) when you need the broader repo-tool map.

# Maintenance Checklist

## Tool Changes

1. Read the existing tool end to end.
2. Remove hard-coded workspace assumptions when Cargo or the filesystem can answer the question.
3. Add structured output when the tool may be chained by future scripts.
4. Smoke-test success and failure paths, not just happy-path help text.
5. For public GitHub or release tooling, test against the live unauthenticated API surface the repo will actually use.

## Skill Changes

1. Edit the checked-in canonical copy under `nex-skills/`.
2. Put detail in `references/`.
3. Keep `agents/openai.yaml` aligned with `SKILL.md`.
4. Run `quick_validate.py` on the repo-managed skill folder.
5. Run `python tools/sync_nex_skills.py` to refresh the installed local copy.

## Rename Hygiene

1. Use `python tools/workspace_doctor.py --legacy-scan` to build a reproducible hit list.
2. Distinguish intentional legacy aliases from stale branding.
3. Keep tool compatibility in place when renaming public-facing files or directories.

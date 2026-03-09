# Maintenance Checklist

## Tool Changes

1. Read the existing tool end to end.
2. Remove hard-coded workspace assumptions when Cargo or the filesystem can answer the question.
3. Add structured output when the tool may be chained by future scripts.
4. Smoke-test success and failure paths, not just happy-path help text.

## Skill Changes

1. Edit the checked-in canonical copy under `codex-skills/`.
2. Put detail in `references/`.
3. Keep `agents/openai.yaml` aligned with `SKILL.md`.
4. Run `quick_validate.py` on the repo-managed skill folder.
5. Run `python tools/sync_codex_skills.py` to refresh the installed local copy.

## Rename Hygiene

1. Use `python tools/workspace_doctor.py --legacy-scan` to build a reproducible hit list.
2. Distinguish intentional legacy aliases from stale branding.
3. Avoid renaming historical `.docx` spec files unless the surrounding tooling is updated in the same slice.

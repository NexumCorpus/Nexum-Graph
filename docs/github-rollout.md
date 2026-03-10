# GitHub Rollout Modes

Use Nexum Graph in three stages, depending on how much merge pressure you want to apply immediately.

## Advisory

Best when a team is first evaluating semantic checks and wants visibility without blocking merges.

```yaml
name: Semantic Check

on:
  pull_request:

jobs:
  semantic-check:
    uses: NexumCorpus/Nexum-Graph/.github/workflows/reusable-semantic-check.yml@v0.1.0
    with:
      format: github
      gate-mode: advisory
      post-pr-comment: true
      upload-sarif: true
```

What this does:
- Keeps the PR comment and artifacts up to date
- Uploads SARIF findings into GitHub code scanning
- Never fails the workflow for semantic conflicts

Use this when you need data before you ask the team to respect the gate.

## Errors-Only

Best when you want to block obvious semantic breakage but keep warning-level drift advisory.

```yaml
name: Semantic Check

on:
  pull_request:

jobs:
  semantic-check:
    uses: NexumCorpus/Nexum-Graph/.github/workflows/reusable-semantic-check.yml@v0.1.0
    with:
      format: github
      gate-mode: errors-only
      post-pr-comment: true
      upload-sarif: true
```

What this does:
- Blocks merges when Nexum Graph finds blocking semantic conflicts
- Keeps warning-level conflicts visible in the PR comment, SARIF upload, and artifacts
- Gives teams a lower-friction path to branch protection than full strict mode

Use this when you want a real merge gate, but only for clearly unsafe changes.

## Strict

Best when semantic drift itself should stop the merge.

```yaml
name: Semantic Check

on:
  pull_request:

jobs:
  semantic-check:
    uses: NexumCorpus/Nexum-Graph/.github/workflows/reusable-semantic-check.yml@v0.1.0
    with:
      format: github
      gate-mode: strict
      post-pr-comment: true
      upload-sarif: true
```

What this does:
- Blocks merges on both errors and warnings
- Treats semantic review as a required branch-protection gate
- Keeps the sticky PR comment and code-scanning surface aligned with the enforced policy

Use this when the team is already relying on multi-agent or AI-heavy workflows and semantic drift needs to be stopped before merge.

## Optional Toggles

Turn off the sticky PR comment:

```yaml
post-pr-comment: false
```

Turn off SARIF upload if you only want artifacts and PR comments:

```yaml
upload-sarif: false
```

Keep the report artifact under a custom name:

```yaml
artifact-name: semantic-review
```

## What You Get

Every mode produces:
- A markdown or text summary from `nex check`
- An HTML review artifact
- A machine-readable insights JSON file

When `upload-sarif: true`, the workflow also emits a SARIF report that GitHub code scanning can render as native findings.

## Operational Checks

After installing the workflow, use the CLI to verify rollout posture locally or in CI:

```bash
nex github status
nex github status --require-current
nex github status --require-current --min-gate-mode errors-only
nex github status --require-current --min-gate-mode errors-only --require-pr-comment --require-sarif
```

Those checks distinguish:
- missing workflow
- custom workflow
- outdated Nexum Graph pin
- advisory-only gate
- limited review surface (PR comment or SARIF disabled)
- fully branch-protection-ready rollout

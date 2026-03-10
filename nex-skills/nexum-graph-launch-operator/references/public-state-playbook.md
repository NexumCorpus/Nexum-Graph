# Public State Playbook

## Use This Skill When

- a release tag exists but the GitHub Release page is incomplete
- Actions is still building and you need to know whether to wait or fix
- README/install claims may be ahead of what GitHub visibly exposes
- you are doing launch-day repo trust work

## Core Commands

- Public summary:
  `python tools/github_release_watch.py public-summary`
- One-shot release check:
  `python tools/github_release_watch.py release-status --tag v0.1.0`
- Watch a release to completion:
  `python tools/github_release_watch.py release-status --tag v0.1.0 --wait-seconds 900 --poll-interval 30`

## Trust Order

1. Repo state is accurate.
2. Local verification is green.
3. Tag exists on GitHub.
4. Release workflow completes successfully.
5. GitHub Release object exists with the expected assets.
6. README and public posts describe only what is actually available.

## Expected Release Assets

- `nexum-graph-vX.Y.Z-x86_64-unknown-linux-gnu.tar.gz`
- `nexum-graph-vX.Y.Z-x86_64-pc-windows-msvc.zip`
- `nexum-graph-vX.Y.Z-x86_64-apple-darwin.tar.gz`
- `nexum-graph-vX.Y.Z-aarch64-apple-darwin.tar.gz`
- `nexum-graph-vscode-X.Y.Z.vsix`
- `SHA256SUMS.txt`

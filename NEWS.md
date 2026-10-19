# Nexum Graph News

Latest product-facing updates for the public repo.

## v0.2.0 (2026-10-19)

First release since v0.1.0 (March 2026). Everything below was on `main` only until now.

- **Correctness (Night Adversary 0, [#5](https://github.com/NexumCorpus/Nexum-Graph/pull/5)).** Dependency extraction
  gave every file but the first zero edges; conflict detection reported callers that a 3-way merge supersedes; bare
  Python names collided across modules. On 30 real merged AI-agent PRs with planted breaks: real breaks caught
  6/7 -> 9/9 (git alone: 4/9), false alarms on controls 1/29 -> 0/30. Small n, one adversary run.
- **GitHub pull-request gate.** `nex github init` / `nex github status` (with `--require-current`, `--min-gate-mode`,
  `--require-pr-comment`, `--require-sarif`), `insights-json` and `sarif` output formats, and the published Action
  `uses: NexumCorpus/Nexum-Graph@v0.2.0`. The Action file failed to load before this release (YAML block broken by a
  heredoc); fixed.
- **Build and CI.** Build bottlenecks removed, intent-kind escalation hardened, macOS CI moved off the retired
  macos-13 runner. 280 Rust tests pass on Linux, Windows and macOS.
- **Known limits.** Python module paths are not part of qualified names, so `DeletedDependency`,
  `ConcurrentBodyEdit` and `NamingCollision` still match on bare names. Extraction covers TypeScript, Python and Rust.

Built by Claude, directed by Nexum Corpus.

## Latest

- GitHub rollout posture is now explicit: `nex github status` reports whether a repo is missing the PR gate, using a custom workflow, pinned to an old release, still advisory-only, missing review surfaces, or fully branch-protection ready
- `nex github status` can now enforce rollout policy in local checks or CI with `--require-current`, `--min-gate-mode`, `--require-pr-comment`, and `--require-sarif`
- `nex start` now understands the same GitHub rollout stages and points operators at the exact upgrade command for stale, advisory, or partially configured PR gates
- One-step installers for macOS, Linux, and Windows via [install.sh](./install.sh) and [install.ps1](./install.ps1)
- GitHub Release automation, bundled binaries, checksum manifests, and packaged VS Code extension artifacts
- A published semantic-check GitHub Action in [action.yml](./action.yml)
- Visual HTML reports for `nex demo`, `nex start`, and `nex check`, plus PR artifact upload in [semantic-check.yml](./.github/workflows/semantic-check.yml)
- Canonical generated repo facts in [README.md](./README.md), enforced by [tools/project_facts.py](./tools/project_facts.py)
- Public Markdown exports of the implementation spec and whitepaper under [docs](./docs)
- Local merge protection with `nex check --install-hook`
- Guided first run with `nex start`

## Start Here

1. Install Nexum Graph

```bash
curl -fsSL https://raw.githubusercontent.com/NexumCorpus/Nexum-Graph/main/install.sh | sh
```

Windows PowerShell:

```powershell
irm https://raw.githubusercontent.com/NexumCorpus/Nexum-Graph/main/install.ps1 | iex
```

2. Run the guided first run

```bash
nex start
```

3. Install the merge guard

```bash
nex check --install-hook
nex github init --gate-mode errors-only
nex github status --require-current --min-gate-mode errors-only --require-pr-comment --require-sarif
```

4. Explore the current semantic diff

```bash
nex diff HEAD~1 HEAD
```

5. Turn on the local coordination server when you are ready

```bash
nex auth init --agent alice --agent bob
nex serve --host 127.0.0.1 --port 4000
```

## For Teams

- Use [action.yml](./action.yml) in GitHub Actions to gate pull requests with `nex check`
- Use [RELEASING.md](./RELEASING.md) to cut tagged releases with bundled binaries
- Use [README.md](./README.md) for the full product walkthrough

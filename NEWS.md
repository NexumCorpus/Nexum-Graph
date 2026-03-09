# Nexum Graph News

Latest product-facing updates for the public repo.

## Latest

- One-step installers for macOS, Linux, and Windows via [install.sh](./install.sh) and [install.ps1](./install.ps1)
- GitHub Release automation, bundled binaries, checksum manifests, and packaged VS Code extension artifacts
- A published semantic-check GitHub Action in [action.yml](./action.yml)
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

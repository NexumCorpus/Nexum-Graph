# Nexum Graph VS Code Extension

This extension starts `nex-lsp` for JavaScript, TypeScript, Python, and Rust workspaces and surfaces Nexum Graph overlays inside VS Code.

What you get:

- lock-aware code lenses from `nex-lsp`
- validation diagnostics merged with upstream diagnostics
- a command to inspect semantic diff for the current file
- per-language upstream proxy configuration

## Fastest install

1. Install Nexum Graph binaries from the latest release.

macOS / Linux:

```bash
curl -fsSL https://raw.githubusercontent.com/NexumCorpus/Nexum-Graph/main/install.sh | sh
```

Windows PowerShell:

```powershell
irm https://raw.githubusercontent.com/NexumCorpus/Nexum-Graph/main/install.ps1 | iex
```

2. Download `nexum-graph-vscode-X.Y.Z.vsix` from the latest [GitHub Release](https://github.com/NexumCorpus/Nexum-Graph/releases).
3. Install the `.vsix` from VS Code:

   1. Open Extensions
   2. Select `...`
   3. Choose `Install from VSIX...`
   4. Pick the downloaded package

4. Make sure `nex-lsp` is on your `PATH`, or point the extension at it with `nexumGraph.nexPath`.

## Prerequisites

Install an upstream server if you want proxied standard LSP behavior through `nex-lsp`:

- TypeScript / JavaScript: `npm install -g typescript-language-server typescript`
- Python: `npm install -g pyright`
- Rust: install `rust-analyzer`

If an upstream command is missing, the extension falls back to overlay-only mode for that language instead of failing.

## Build from source

```bash
cargo build --release -p nex-lsp -p nex-cli
cd extensions/vscode
npm install
npm run compile
npm run package
```

## Key Settings

- `nexumGraph.nexPath`
- `nexumGraph.baseRef`
- `nexumGraph.eventPollMs`
- `nexumGraph.typescript.*`
- `nexumGraph.python.*`
- `nexumGraph.rust.*`

Set an upstream command to an empty string to run `nex-lsp` without proxying that language server.

## Commands

- `Nexum Graph: Show Semantic Diff for Current File`
- `Nexum Graph: Restart Language Clients`
- `Nexum Graph: Show Output`

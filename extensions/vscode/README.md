# Nexum Graph VS Code Extension

This extension starts `nex-lsp` for JavaScript, TypeScript, Python, and Rust workspaces and surfaces Nexum Graph overlays inside VS Code.

What you get:

- lock-aware code lenses from `nex-lsp`
- validation diagnostics merged with upstream diagnostics
- a command to inspect semantic diff for the current file
- per-language upstream proxy configuration

## Prerequisites

1. Build the Rust binaries:

```bash
cargo build --release -p nex-lsp -p nex-cli
```

2. Make `nex-lsp` available on your `PATH`, or point the extension at it with `nexumGraph.nexPath`.

3. Install an upstream server if you want proxied standard LSP behavior through `nex-lsp`:

- TypeScript / JavaScript: `npm install -g typescript-language-server typescript`
- Python: `npm install -g pyright`
- Rust: install `rust-analyzer`

If an upstream command is missing, the extension falls back to overlay-only mode for that language instead of failing.

## Install From Source

```bash
cd extensions/vscode
npm install
npm run compile
npm run package
```

Install the generated `.vsix` from VS Code:

1. Open Extensions
2. Select `...`
3. Choose `Install from VSIX...`
4. Pick the generated package

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

# Releasing Nexum Graph

Nexum Graph releases are GitHub Releases driven by semver tags in `vX.Y.Z` form.

## Before tagging

1. Keep the release version in sync across:
   - `crates/nex-cli/Cargo.toml`
   - `crates/nex-lsp/Cargo.toml`
   - `extensions/vscode/package.json`
2. Run the release checks locally:

```bash
python tools/release_tools.py assert-version-parity --tag vX.Y.Z
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo fmt --check
npm --prefix extensions/vscode ci
npm --prefix extensions/vscode test
npm --prefix extensions/vscode run package
```

## Cutting a release

1. Commit the version bump and release notes changes.
2. Create the annotated tag:

```bash
git tag -a vX.Y.Z -m "Nexum Graph vX.Y.Z"
git push origin main --follow-tags
```

3. Wait for the `Release` workflow to finish.
4. Verify the GitHub Release contains:
   - one bundle per supported target
   - `nexum-graph-vscode-X.Y.Z.vsix`
   - `SHA256SUMS.txt`
5. Spot-check install and automation flows with:
   - `install.sh`
   - `install.ps1`
   - `nex demo`
   - `uses: NexumCorpus/Nexum-Graph@vX.Y.Z` on a test pull request
   - `nex check --install-hook` in a local clone
   - VS Code `Install from VSIX...`

## Manual rerun

If you need to rebuild assets for an existing tag, run the `Release` workflow manually and provide the exact tag, for example `v0.1.0`.

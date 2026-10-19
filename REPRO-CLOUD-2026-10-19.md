# Stranger-machine reproduction: cloud, 2026-10-19

**Result: PASS.** Every documented step ran on a clean Linux machine. 280 Rust tests passed, 0 failed, 9 ignored.
`nex check` caught all 3 planted breaks that `git merge` let through. The v0.2.0 Linux asset's checksum matched,
and the asset printed `nex 0.2.0`. The README has 7 gaps, listed below. Two of them matter: the README shows no sample
output to compare against, and the Linux binary needs glibc ≥ 2.38.

## Machine

- Claude Code cloud session (fresh container), Ubuntu 24.04.5 LTS, glibc 2.39, x86_64, 4 vCPU, 15 GB RAM
- Preinstalled: rustc/cargo 1.97.0, node 22.22.0, npm 10.9.4, Python 3, curl, git, gh
- Commit under test: `ee95579` (main = tag v0.2.0 commit, "Release v0.2.0: version bump, NEWS section, README release status")
- Source steps ran in a fresh `git clone` in a scratch directory, not in a pre-existing checkout.
- Times are wall-clock seconds from `date +%s.%N` around each command. The build and test times are from cold caches.

## Results

| Step | Command | Exit | Seconds | Matches docs? |
|---|---|---|---|---|
| 1a. One-step install | `curl -fsSL https://raw.githubusercontent.com/NexumCorpus/Nexum-Graph/main/install.sh \| sh` | 0 | 3.5 | Yes. Installed `nex` and `nex-lsp` 0.2.0 to `~/.local/bin` (already on PATH here) |
| 1b. Source: clone | `git clone https://github.com/NexumCorpus/Nexum-Graph.git` | 0 | 1.0 | Yes |
| 1c. Source: build | `cargo build --release` | 0 | 315.3 | Yes. No warnings to note |
| 1d. Source: extension deps | `npm --prefix extensions/vscode ci` | 0 | 2.2 | Yes. npm reports 1 high-severity audit finding |
| 1e. Source: extension package | `npm --prefix extensions/vscode run package` | 0 | 13.6 | Yes. Produced `nexum-graph-vscode-0.2.0.vsix` (8 files, 8.79 KB). See gap 5 |
| 2a. Tests | `cargo test --workspace` | 0 | 270.6 | **280 passed, 0 failed, 9 ignored** across 50 result lines. Matches "280 Rust". All 9 ignored are doctests in `nex-cli/coordination_pipeline.rs` (3) and `nex-validate/validator.rs` (6) |
| 2b. Tests, as in "Evidence" | `cargo test --workspace --locked` | 0 | 5.5 (warm) | Same: 280 / 0 / 9. The lockfile is current |
| 2c. Python tool suite | `python tools/tool_selftest.py` | **1** | 35.7 | Partly. The unit tests ran 45 tests: OK, 1 skipped (matches "45 Python"). The script exits 1 because of the Codex skill checks. See gap 6 |
| 3a. Quickstart | `nex start` (in the clone) | 0 | 2.7 | Yes. It printed every item the README lists: languages, files (98), units (1372), edges (8836), HEAD~1→HEAD diff, hook/auth state, next commands |
| 3b. Quickstart | `nex diff HEAD~1 HEAD` | 0 | 1.6 | Ran (6 modified units). The README shows no sample output to compare against |
| 3c. Quickstart, verbatim | `nex check feature/a feature/b` | **1** | 0.02 | **No.** `error: git error: revspec 'feature/a' not found`. These branches are placeholders. See gap 1 |
| 3d. Quickstart | `nex demo --format html > nex-demo-report.html` | 0 | 2.4 | Yes. 8.4 KB HTML report |
| 3e. `nex check` on a planted fixture (added by me) | `nex check feature/a feature/b` | 1 (expected) | 0.02 | Yes. 3 errors, risk 100/100. `git merge-tree` merges the same branches cleanly (exit 0). Details below |
| 3f. `nex check` control (added by me) | `nex check feature/b feature/c` | 0 | 0.02 | Yes. "Clean semantic check (0/100)", no false alarm |
| 4a. Release, as written in the brief | `gh release view v0.2.0 --repo NexumCorpus/Nexum-Graph` | 1 | 1.5 | Did not run. The sandbox blocks GitHub GraphQL with a 403, so this failure is not the repo's fault |
| 4b. Release metadata | `gh api repos/NexumCorpus/Nexum-Graph/releases/tags/v0.2.0` | 0 | 0.4 | Yes. Published 2026-10-19T19:11:11Z, not a draft or prerelease. 6 assets: linux-gnu, 2× darwin, windows zip, `.vsix`, `SHA256SUMS.txt`. This matches the README's "Release assets ship" list |
| 4c. Download Linux asset | `curl -fsSLO …/releases/download/v0.2.0/nexum-graph-v0.2.0-x86_64-unknown-linux-gnu.tar.gz` (+ `SHA256SUMS.txt`) | 0 | 1.3 | Yes. 14,729,634 bytes (`gh release download` is blocked here for the same GraphQL reason) |
| 4d. Verify checksum | `sha256sum -c --ignore-missing SHA256SUMS.txt` | 0 | 0.05 | `nexum-graph-v0.2.0-x86_64-unknown-linux-gnu.tar.gz: OK` |
| 4e. Run it | `nex --version` (from the extracted asset) | 0 | 0.01 | **`nex 0.2.0`**. `nex-lsp --version` → `nex-lsp 0.2.0`. The binary is byte-identical (`cmp`) to the one the installer put in step 1a |

Total measured command time is about 660 s (~11 min). Most of it is the cold release build (315 s) and the cold test
build+run (271 s).

### Fixture for step 3e

The README does not ship a two-branch example, so I built one. It has one base commit on `main` with `lib.py`
(`compute_total(items)`, `format_name(first, last)`) and `app.py`.

- `feature/a`: `compute_total(items, tax_rate)` (signature change), and `format_name` renamed to `full_name`.
- `feature/b`: new `report.py` that calls `compute_total([4, 5])` and `format_name("Ada", "Lovelace")`.
- `feature/c` (control): adds an unrelated function to `app.py`.

`git merge-tree --write-tree feature/a feature/b` exits 0, so git merges these with no conflict. `nex check feature/a feature/b` output:

```
Merge risk: High merge risk (100/100)
[ERROR] SignatureMismatch: Branch A changed signature of `compute_total` but branch B calls it
[ERROR] SignatureMismatch: Branch A changed signature of `compute_total` but branch B calls it
[ERROR] DeletedDependency: Branch A deleted `format_name` but branch B depends on it via `report`
Exit code: 1
```

The `compute_total` mismatch is reported twice for one call site. My guess is that the `from lib import` line counts as a
second reference. I did not check this. The rename is reported as a deletion, which is correct in effect.

## Doc gaps found

1. **`nex check feature/a feature/b` fails verbatim.** The Quickstart, "Choose Your Path" and "Core Workflows" sections
   all show it, but the branches are placeholders. Nothing in the repo creates them, and the README never says to swap in
   your own branch names. A stranger running it gets `revspec 'feature/a' not found` (exit 1). Suggestion: say
   "replace with two of your branches", or ship a tiny script that builds a fixture like the one above.
2. **The README shows no sample output.** "Check that the output matches what the README shows" cannot be done for
   `nex start`, `nex diff` or `nex check`. The README lists the *kinds* of things `nex start` prints, and the real output
   has all of them. It shows no exit codes, so a stranger has no reference transcript.
3. **Linux binary needs glibc ≥ 2.38, and the README doesn't say so.** `objdump -T nex` shows `GLIBC_2.38`
   (`__isoc23_strtol`). It runs here (glibc 2.39). I expect it to fail on Ubuntu 22.04 (2.35) and Debian 12 (2.36), which
   are common CI and server images. I did not test those distros. "Bundled for Linux" should state the floor, or the
   release should build on an older base or use musl.
4. **CLI command table lists 14 commands and omits `nex github`.** The facts block says "15 CLI commands", and
   `nex --help` does list 15 (not counting `help`). `nex github` is documented further down but is missing from the
   table. `tools/project_facts.py check-readme` passes, so that check does not cover the table.
5. **The extension package step is not pinned.** `npm run package` runs `npx @vscode/vsce package`. vsce is not a
   devDependency, so npx downloaded `@vscode/vsce@4.0.0` at run time ("package was not found and will be installed").
   The `.vsix` build therefore depends on whatever vsce is current that day. `npm ci` also reports 1 high-severity
   vulnerability.
6. **`tools/tool_selftest.py` (README "Developer Workflow") exits 1 on a stranger's machine.** All its code checks pass:
   py_compile OK, 45 unit tests OK with 1 skipped, workspace_doctor OK, fuzz `cargo check` OK. It fails only on
   `skill_repo_check` / `repo_skill:*`, which expect repo skills installed under `$CODEX_HOME`. The README doesn't say this
   step needs Codex setup, or that this failure is expected without it.
7. **Prerequisites are not listed.** The source build needs Rust (any recent stable; 1.97.0 used here), Node/npm
   (for the extension), a C toolchain (for `git2`/tree-sitter), and Python 3 (for the tools). None of these are stated.
   The cloud image had them all, so nothing had to be added *here*, but a truly bare machine would stop at the first
   `cargo` command.

Not a doc gap, but out of scope for a README check: the brief cites "23/23 caught" for the Night Adversary. The README's
Evidence section claims 9/9 real breaks and 0/30 false alarms (Night Adversary 0). I did not rerun the adversary, and I
saw no rerunnable artifact for either number linked from the README.

## Cost

I can't see this session's dollar cost from inside the container. The steward should read it from the billing side.
For scale: about 11 min of measured command time on 4 vCPU, plus agent overhead for reading, the fixture and this write-up.

PASS

from __future__ import annotations

import hashlib
import json
import os
import shlex
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


TOOLS_DIR = Path(__file__).resolve().parent
WORKSPACE = TOOLS_DIR.parent
VERIFY_TREE = TOOLS_DIR / "verify_tree.py"
CHECK = (
    "from pathlib import Path; import sys; "
    "sys.exit(Path('first.txt').read_text().strip() != 'passing')"
)


class VerifyTreeTests(unittest.TestCase):
    def setUp(self) -> None:
        temporary = tempfile.TemporaryDirectory(prefix="verify-tree-test-", dir=WORKSPACE)
        self.addCleanup(temporary.cleanup)
        self.directory = Path(temporary.name).resolve(strict=True)
        self.assertEqual(self.directory.parent, WORKSPACE)
        self.repo = self.directory / "repo"
        self.repo.mkdir()
        self.receipt = self.directory / "receipt.json"
        self.environment = {
            name: value for name, value in os.environ.items()
            if not name.upper().startswith("GIT_")
        }
        self.environment.update(
            GIT_CONFIG_GLOBAL=os.devnull,
            GIT_CONFIG_NOSYSTEM="1",
            GIT_AUTHOR_NAME="Tree Receipt Test",
            GIT_AUTHOR_EMAIL="tree-test@example.invalid",
            GIT_COMMITTER_NAME="Tree Receipt Test",
            GIT_COMMITTER_EMAIL="tree-test@example.invalid",
        )
        self.git("init", "--quiet", "--template=")

    def run_command(
        self, *command: str, expected: int = 0
    ) -> subprocess.CompletedProcess[str]:
        result = subprocess.run(
            command, cwd=self.repo, env=self.environment, capture_output=True,
            text=True, encoding="utf-8", errors="replace", timeout=30,
        )
        self.assertEqual(result.returncode, expected, result.stdout + result.stderr)
        return result

    def git(self, *args: str, expected: int = 0) -> subprocess.CompletedProcess[str]:
        return self.run_command("git", *args, expected=expected)

    def verify(self, *args: str, expected: int = 0) -> dict:
        result = self.run_command(
            sys.executable, "-B", str(VERIFY_TREE), *args, expected=expected,
        )
        return json.loads(result.stdout)

    def stage(self, name: str, content: str) -> None:
        (self.repo / name).write_bytes(content.encode("ascii"))
        self.git("add", "--", name)

    def run_check(self, *options: str, expected: int = 0) -> dict:
        return self.verify(
            "run", "--scratch", str(self.directory), "--receipt", str(self.receipt), *options,
            "--", sys.executable, "-B", "-c", CHECK, expected=expected,
        )

    def test_unstaged_pass_does_not_mask_staged_failure(self) -> None:
        self.stage("first.txt", "failing")
        (self.repo / "first.txt").write_bytes(b"passing")
        self.run_command(sys.executable, "-B", "-c", CHECK)

        report = self.run_check(expected=1)

        self.assertEqual(report["tree"], self.git("write-tree").stdout.strip())
        self.assertEqual(report["status"], "failed")
        self.assertEqual(report["test_exit"], 1)
        match = self.verify("match", "--index", str(self.receipt), expected=1)
        self.assertTrue(match["same_tree"])
        self.assertFalse(match["matched"])

    def test_passing_staged_tree_matches_commit_with_unstaged_edits(self) -> None:
        self.stage("first.txt", "passing")
        (self.repo / "first.txt").write_bytes(b"failing")

        report = self.run_check()
        self.git("commit", "--quiet", "-m", "Test staged content")
        match = self.verify("match", str(self.receipt), "HEAD")

        self.assertEqual(report["status"], "passed")
        self.assertEqual(match["committed_tree"], report["tree"])
        self.assertTrue(match["matched"])
        self.assertEqual((self.repo / "first.txt").read_bytes(), b"failing")

    def test_frozen_tree_runs_while_index_is_locked(self) -> None:
        self.stage("first.txt", "passing")
        self.git("commit", "--quiet", "-m", "Keep a passing tree")
        tree = self.git("rev-parse", "HEAD^{tree}").stdout.strip()
        self.stage("first.txt", "failing")
        lock = self.repo / ".git" / "index.lock"
        lock.touch(exist_ok=False)
        try:
            self.git("write-tree", expected=128)
            report = self.run_check("--tree", tree)
        finally:
            lock.unlink()

        self.assertEqual(report["status"], "passed")
        self.assertEqual(report["tree"], tree)
        self.assertTrue(self.verify("match", str(self.receipt), "HEAD")["matched"])
        match = self.verify("match", "--index", str(self.receipt), expected=1)
        self.assertFalse(match["same_tree"])
        self.assertTrue(match["passed_receipt"])

    def test_frozen_tree_reads_the_named_object(self) -> None:
        self.stage("first.txt", "passing")
        tree = self.git("write-tree").stdout.strip()
        self.stage("first.txt", "failing")
        replacement = self.git("write-tree").stdout.strip()
        self.git("replace", tree, replacement)

        report = self.run_check("--tree", tree.upper())

        self.assertEqual(report["status"], "passed")
        self.assertEqual(report["tree"], tree)

    def test_compose_uses_proposals_instead_of_the_shared_index(self) -> None:
        source = "opening\nx1\nx2\nx3\nx4\nx5\nclosing\n"
        self.stage("notes.txt", source)
        self.git("commit", "--quiet", "-m", "Base")
        base = self.git("rev-parse", "HEAD^{tree}").stdout.strip()
        self.stage("notes.txt", source.replace("opening", "first"))
        left = self.git("write-tree").stdout.strip()
        self.stage("notes.txt", source.replace("closing", "second"))
        right = self.git("write-tree").stdout.strip()
        self.stage("foreign.txt", "another session's work")
        original_index = self.git("write-tree").stdout.strip()

        composed = self.verify("compose", "--base", base, left, right)

        self.assertEqual(composed["status"], "composed")
        self.assertEqual(composed["input_trees"], [left, right])
        self.assertEqual(self.git("write-tree").stdout.strip(), original_index)
        expected = source.replace("opening", "first").replace("closing", "second")
        check = (
            f"from pathlib import Path; assert Path('notes.txt').read_text() == {expected!r}; "
            "assert not Path('foreign.txt').exists()"
        )
        receipt = self.verify(
            "run", "--tree", composed["tree"], "--scratch", str(self.directory),
            "--receipt", str(self.receipt), "--", sys.executable, "-B", "-c", check,
        )
        self.assertEqual(receipt["status"], "passed")
        self.git("read-tree", composed["tree"])
        self.git("commit", "--quiet", "-m", "Land the checked proposals")
        self.assertTrue(self.verify("match", str(self.receipt), "HEAD")["matched"])

    def test_compose_stops_on_an_overlapping_edit(self) -> None:
        trees = []
        for text in ("base", "first", "second"):
            self.stage("notes.txt", text)
            trees.append(self.git("write-tree").stdout.strip())

        report = self.verify("compose", "--base", *trees, expected=1)

        self.assertEqual(report["status"], "conflicted")
        self.assertEqual(report["paths"], ["notes.txt"])
        self.assertNotIn("tree", report)

    def test_timeout_survives_tracked_input_change(self) -> None:
        self.stage("first.txt", "passing")
        check = "from pathlib import Path; import time; Path('first.txt').write_bytes(b'changed'); time.sleep(20)"

        report = self.verify(
            "run", "--scratch", str(self.directory), "--receipt", str(self.receipt), "--timeout", "1",
            "--", sys.executable, "-B", "-c", check, expected=1,
        )

        self.assertEqual(report["status"], "timed_out")
        self.assertIsNone(report["test_exit"])
        self.assertEqual(report["changed_tracked_inputs"], ["first.txt"])
        self.assertEqual((self.repo / "first.txt").read_bytes(), b"passing")
        match = self.verify("match", "--index", str(self.receipt), expected=1)
        self.assertTrue(match["same_tree"])
        self.assertFalse(match["passed_receipt"])

    def test_streaming_preserves_binary_and_empty_blobs(self) -> None:
        payload = bytes(range(256)) * 4097 + b"\n"
        (self.repo / "payload.bin").write_bytes(payload)
        (self.repo / "empty.bin").touch()
        self.git("add", "--", "payload.bin", "empty.bin")
        check = (
            "from pathlib import Path; import hashlib; "
            f"assert hashlib.sha256(Path('payload.bin').read_bytes()).hexdigest() == {hashlib.sha256(payload).hexdigest()!r}; "
            "assert Path('empty.bin').read_bytes() == b''"
        )

        report = self.verify(
            "run", "--scratch", str(self.directory), "--receipt", str(self.receipt),
            "--", sys.executable, "-B", "-c", check,
        )

        self.assertEqual(report["status"], "passed")
        self.assertEqual(report["tracked_files"], 2)

    def test_commit_msg_checks_path_limited_proposal_before_head_moves(self) -> None:
        self.stage("first.txt", "baseline")
        self.stage("second.txt", "baseline")
        self.git("commit", "--quiet", "-m", "Baseline")
        original_head = self.git("rev-parse", "HEAD").stdout.strip()
        self.stage("first.txt", "passing")
        self.stage("second.txt", "updated")
        report = self.run_check()

        hooks = self.directory / "hooks"
        hooks.mkdir()
        hook = hooks / "commit-msg"
        hook_report = self.directory / "hook-report.json"
        command = [
            Path(sys.executable).as_posix(), "-B", VERIFY_TREE.as_posix(),
            "match", "--index", self.receipt.as_posix(),
        ]
        script = "#!/bin/sh\nexec " + shlex.join(command) + " > " + shlex.quote(hook_report.as_posix()) + "\n"
        hook.write_bytes(script.encode("utf-8"))
        hook.chmod(0o755)
        self.git("config", "core.hooksPath", hooks.as_posix())

        result = self.git("commit", "-m", "Partial proposal", "--", "first.txt", expected=1)
        self.assertTrue(hook_report.stat().st_size, result.stdout + result.stderr)
        refused = json.loads(hook_report.read_text(encoding="utf-8"))
        self.assertFalse(refused["matched"])
        self.assertTrue(refused["passed_receipt"])
        self.assertNotEqual(refused["proposed_tree"], report["tree"])
        self.assertEqual(self.git("rev-parse", "HEAD").stdout.strip(), original_head)
        self.assertEqual(self.git("write-tree").stdout.strip(), report["tree"])

        result = self.git("commit", "--quiet", "-m", "Complete proposal")
        self.assertTrue(hook_report.stat().st_size, result.stdout + result.stderr)
        self.assertTrue(json.loads(hook_report.read_text(encoding="utf-8"))["matched"])
        self.assertNotEqual(self.git("rev-parse", "HEAD").stdout.strip(), original_head)
        self.assertTrue(self.verify("match", str(self.receipt), "HEAD")["matched"])


if __name__ == "__main__":
    unittest.main()

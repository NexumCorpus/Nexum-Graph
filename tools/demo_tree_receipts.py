"""Trace a passing proposal, a broken merge, and a verified repair in a disposable Git repo."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile


VERIFIER = Path(__file__).with_name("verify_tree.py").resolve()
PROBE = """\
import json
from pathlib import Path
import runpy

try:
    value = runpy.run_path(str(Path.cwd() / 'caller.py'))['label']()
    report = {'value': value, 'ok': value == 'HELLO'}
except (AttributeError, KeyError, TypeError) as error:
    report = {'error': type(error).__name__, 'ok': False}
Path('report.json').write_bytes(json.dumps(report, sort_keys=True).encode('utf-8'))
raise SystemExit(int(not report['ok']))
"""


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--scratch", type=Path, required=True, help="Existing parent for the disposable run.")
    args = parser.parse_args()
    scratch = Path(tempfile.mkdtemp(prefix="nexum-receipt-demo-", dir=args.scratch.resolve(strict=True)))
    repo = scratch / "fixture"
    repo.mkdir()
    print(f"Scratch directory: {scratch}", flush=True)

    environment = {key: value for key, value in os.environ.items() if not key.startswith("GIT_")}
    environment.update(GIT_CONFIG_GLOBAL=os.devnull, GIT_CONFIG_SYSTEM=os.devnull)

    def command(*argv, expected=0):
        result = subprocess.run(argv, cwd=repo, env=environment, capture_output=True, timeout=30)
        if result.returncode != expected:
            raise RuntimeError(f"Expected exit {expected}, got {result.returncode}: {argv}\n"
                               f"{result.stdout.decode(errors='replace')}{result.stderr.decode(errors='replace')}")
        return result.stdout

    def git(*argv):
        return command("git", *argv).decode().strip()

    def verify(action, *argv, expected=0):
        return json.loads(command(sys.executable, "-I", "-B", str(VERIFIER), action, *argv, expected=expected))

    git("init", "--quiet", "--template=", "--initial-branch=target")
    git("config", "user.name", "Nexum demo")
    git("config", "user.email", "demo@example.invalid")
    (repo / "handler.py").write_bytes(b"def greeting():\n    return 'hello'\n")
    git("add", "--", "handler.py")
    git("commit", "--quiet", "-m", "Base greeting")
    base_commit = git("rev-parse", "HEAD")
    base = git("rev-parse", "HEAD^{tree}")

    (repo / "handler.py").write_bytes(b"def greeting():\n    return {'text': 'hello'}\n")
    git("commit", "--quiet", "-m", "Return a structured greeting", "--", "handler.py")
    target_commit = git("rev-parse", "HEAD")
    target = git("rev-parse", "HEAD^{tree}")

    git("switch", "--quiet", "-c", "proposal", base_commit)
    caller = b"import runpy\n\ndef label():\n    return runpy.run_path('handler.py')['greeting']().upper()\n"
    (repo / "caller.py").write_bytes(caller)
    (repo / "probe.py").write_bytes(PROBE.encode("utf-8"))
    git("add", "--", "caller.py", "probe.py")
    git("commit", "--quiet", "-m", "Add a greeting caller and its probe")
    proposal_commit = git("rev-parse", "HEAD")
    proposal = git("rev-parse", "HEAD^{tree}")
    probe = ("--probe", proposal, "probe.py")

    def run_check(name, tree, expected_exit, expected_report):
        receipt_path = scratch / f"{name}.json"
        verify("run", "--tree", tree, *probe, "--evidence", "report.json", "--scratch", scratch,
               "--receipt", receipt_path, expected=expected_exit)
        receipt = json.loads(receipt_path.read_bytes())
        report = (Path(receipt["snapshot"]) / "report.json").read_bytes()
        digest = hashlib.sha256(report).hexdigest()
        if json.loads(report) != expected_report or receipt["test_exit"] != expected_exit:
            raise RuntimeError(f"Unexpected probe result in {receipt_path}")
        if receipt["evidence"]["sha256"] != digest:
            raise RuntimeError(f"Report does not match {receipt_path}")
        print(f"{name}: {receipt['status']} ({report.decode()})")
        return receipt_path, report

    passing = {"value": "HELLO", "ok": True}
    proposal_receipt, _ = run_check("proposal", proposal, 0, passing)
    composition = verify("compose", "--base", base, target, proposal)
    (scratch / "composition.json").write_text(json.dumps(composition, indent=2) + "\n", encoding="utf-8")
    merged = composition["tree"]
    merge_commit = git("commit-tree", merged, "-p", target_commit, "-p", proposal_commit, "-m", "Clean text merge")
    print("Git composed both proposals with zero text conflicts.")
    run_check("merge", merged, 1, {"error": "AttributeError", "ok": False})
    stale = verify("match", *probe, "--composition", base, proposal, target_commit, proposal_receipt, merge_commit,
                   expected=1)
    if stale["same_tree"] or not stale["composition"]["same_composed_tree"]:
        raise RuntimeError(f"Unexpected stale receipt result: {stale}")
    print("Passing proposal receipt rejected for the merge.")

    git("switch", "--quiet", "-c", "integrated", merge_commit)
    (repo / "caller.py").write_bytes(caller.replace(b"().upper()", b"()['text'].upper()"))
    git("commit", "--quiet", "-m", "Adapt the caller to the structured return", "--", "caller.py")
    repair_commit = git("rev-parse", "HEAD")
    repair = git("rev-parse", "HEAD^{tree}")
    repair_receipt, report = run_check("repair", repair, 0, passing)
    match_options = (*probe, "--composition", merged, repair, merge_commit)
    match = verify("match", *match_options, "--evidence", "report.json", hashlib.sha256(report).hexdigest(),
                   repair_receipt, repair_commit)
    (scratch / "match.json").write_text(json.dumps(match, indent=2) + "\n", encoding="utf-8")
    print("Repair matched the commit, composition, pinned probe, and report bytes.")
    changed = verify("match", *match_options, "--evidence", "report.json", hashlib.sha256(report + b"\n").hexdigest(),
                     repair_receipt, repair_commit, expected=1)
    if changed != {**match, "same_evidence": False, "matched": False}:
        raise RuntimeError(f"Unexpected changed-report result: {changed}")
    print("Adding one byte to the report made the match fail.")


if __name__ == "__main__":
    main()

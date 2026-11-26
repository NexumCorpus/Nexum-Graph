"""Run a check on an exact Git tree and match the receipt before or after a commit."""

import argparse
import hashlib
import json
import os
from pathlib import Path, PureWindowsPath
import stat
import subprocess
import sys
import tempfile


SCHEMA = "nexum-tree-check-v1"
GIT = ["git", "--no-replace-objects"]
BLOCKED_NAMES = {".git", ".env", "auth.json", "config.json"}
BLOB_CHUNK_BYTES = 1024 * 1024
is_reserved_path = getattr(os.path, "isreserved", lambda name: PureWindowsPath(name).is_reserved())


def git(repo, *args):
    result = subprocess.run([*GIT, "-C", str(repo), *args], capture_output=True, timeout=30)
    if result.returncode:
        raise ValueError(f"git {args[0]}: {result.stderr.decode(errors='replace').strip()}")
    return result.stdout


def exact_oid(repo, value, algorithm, kind="tree"):
    oid = value.lower()
    if len(oid) != 2 * hashlib.new(algorithm).digest_size or any(c not in "0123456789abcdef" for c in oid):
        raise ValueError(f"Expected a full {algorithm} {kind} object ID")
    if git(repo, "cat-file", "-t", oid).decode().strip() != kind:
        raise ValueError(f"Expected a Git {kind} object")
    return oid


def compose_trees(repo, base, left, right, algorithm):
    base = exact_oid(repo, base, algorithm)
    left = exact_oid(repo, left, algorithm)
    right = exact_oid(repo, right, algorithm)
    result = subprocess.run(
        [*GIT, "-C", str(repo), "merge-tree", "--write-tree", "--name-only", "--no-messages",
         "-z", "--merge-base", base, left, right],
        capture_output=True, timeout=30,
    )
    report = {"base_tree": base, "input_trees": [left, right], "object_format": algorithm}
    if result.returncode == 1:
        # A conflicted result also begins with an object ID. It cannot be used as a candidate.
        paths = [path.decode(errors="replace") for path in result.stdout.split(b"\0")[1:] if path]
        return {**report, "status": "conflicted", "paths": paths}, 1
    if result.returncode:
        raise ValueError(f"git merge-tree: {result.stderr.decode(errors='replace').strip()}")
    tree = result.stdout.rstrip(b"\0\r\n").decode()
    return {**report, "status": "composed", "tree": exact_oid(repo, tree, algorithm)}, 0


def check_compose(args):
    algorithm = git(args.repo, "rev-parse", "--show-object-format").decode().strip()
    return compose_trees(args.repo, args.base, args.left, args.right, algorithm)


def read_entries(repo, tree):
    entries = {}
    windows_paths = {}
    for record in git(repo, "ls-tree", "-r", "-z", tree).split(b"\0"):
        if not record:
            continue
        info, name = record.decode("utf-8").split("\t", 1)
        mode, kind, oid = info.split()
        parts = name.split("/")
        if mode not in ("100644", "100755") or kind != "blob":
            raise ValueError(f"Unsupported tree entry: {name} ({mode})")
        if any(part in ("", ".", "..") or part.casefold() in BLOCKED_NAMES or "\\" in part or ":" in part
               or (os.name == "nt" and (part.endswith((" ", ".")) or is_reserved_path(part)))
               for part in parts):
            raise ValueError(f"Refusing tree path: {name}")
        if os.name == "nt":
            for index in range(1, len(parts) + 1):
                prefix = "/".join(parts[:index])
                if windows_paths.setdefault(prefix.casefold(), prefix) != prefix:
                    raise ValueError(f"Case-colliding tree path: {name}")
        entries[name] = (mode, oid)
    return entries


def blob_id(source, size, algorithm, output=None):
    digest = hashlib.new(algorithm, f"blob {size}\0".encode("ascii"))
    remaining = size
    while remaining:
        chunk = source.read(min(remaining, BLOB_CHUNK_BYTES))
        if not chunk:
            return None
        digest.update(chunk)
        if output is not None:
            output.write(chunk)
        remaining -= len(chunk)
    return digest.hexdigest()


def materialize(repo, directory, entries, algorithm):
    command = [*GIT, "-C", str(repo), "cat-file", "--batch"]
    with subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE) as process:
        with process.stdin as requests, process.stdout as blobs:
            for name, (mode, oid) in entries.items():
                requests.write(oid.encode("ascii") + b"\n")
                requests.flush()
                header = blobs.readline().split()
                if len(header) != 3 or header[:2] != [oid.encode("ascii"), b"blob"] or not header[2].isdigit():
                    raise ValueError(f"Invalid response for staged blob: {name}")
                target = directory / name
                target.parent.mkdir(parents=True, exist_ok=True)
                with target.open("xb") as file:
                    if blob_id(blobs, int(header[2]), algorithm, output=file) != oid or blobs.read(1) != b"\n":
                        raise ValueError(f"Blob disagrees with the staged tree: {name}")
                if os.name != "nt":
                    target.chmod(0o755 if mode == "100755" else 0o644)
        if process.wait(timeout=30):
            raise ValueError("Git exited unsuccessfully while reading staged blobs")


def unchanged(directory, name, mode, oid, algorithm):
    path = directory
    try:
        for part in ("", *name.split("/")):
            path = path / part
            info = path.lstat()
            reparse = getattr(info, "st_file_attributes", 0) & stat.FILE_ATTRIBUTE_REPARSE_POINT
            if stat.S_ISLNK(info.st_mode) or reparse:
                return False
    except (FileNotFoundError, NotADirectoryError):
        return False
    executable = bool(info.st_mode & stat.S_IXUSR)
    if not stat.S_ISREG(info.st_mode) or (os.name != "nt" and executable != (mode == "100755")):
        return False
    with path.open("rb") as file:
        return blob_id(file, info.st_size, algorithm) == oid and not file.read(1)


def probe_binding(repo, spec, algorithm):
    source, path = spec
    tree = exact_oid(repo, source, algorithm)
    mode_and_blob = read_entries(repo, tree).get(path)
    if mode_and_blob is None:
        raise ValueError(f"No probe at {path} in tree {tree}")
    mode, blob = mode_and_blob
    return {"tree": tree, "path": path, "mode": mode, "blob": blob}


def check_run(args):
    command = args.command[1:] if args.command[:1] == ["--"] else args.command
    if args.timeout < 1 or bool(command) == bool(args.probe):
        raise ValueError("Pass a positive timeout and either a command after -- or --probe TREE PATH")
    if args.receipt.exists():
        raise ValueError(f"Receipt already exists: {args.receipt}")

    algorithm = git(args.repo, "rev-parse", "--show-object-format").decode().strip()
    if args.tree is None:
        tree = git(args.repo, "write-tree").decode().strip()
    else:
        tree = exact_oid(args.repo, args.tree, algorithm)
    entries = read_entries(args.repo, tree)
    scratch = args.scratch.resolve(strict=True)
    probe = probe_binding(args.repo, args.probe, algorithm) if args.probe else None
    if probe is not None:
        probe_folder = Path(tempfile.mkdtemp(prefix="nexum-probe-", dir=scratch))
        probe_path = probe["path"]
        materialize(args.repo, probe_folder, {probe_path: (probe["mode"], probe["blob"])}, algorithm)
        command = [sys.executable, "-I", "-B", "-X", "utf8", str(probe_folder / probe_path)]
    snapshot = Path(tempfile.mkdtemp(prefix="nexum-tree-", dir=scratch))
    materialize(args.repo, snapshot, entries, algorithm)

    # Do not pass Git's source index or worktree overrides into the check.
    environment = {name: value for name, value in os.environ.items() if not name.startswith("GIT_")}
    environment["GIT_CEILING_DIRECTORIES"] = str(snapshot.parent)
    try:
        result = subprocess.run(command, cwd=snapshot, env=environment, timeout=args.timeout,
                                stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        exit_code = result.returncode
        status = "passed" if exit_code == 0 else "failed"
    except subprocess.TimeoutExpired:
        exit_code, status = None, "timed_out"

    changed = [name for name, (mode, oid) in entries.items() if not unchanged(snapshot, name, mode, oid, algorithm)]
    if changed and status != "timed_out":
        status = "tracked_inputs_changed"

    receipt = {"schema": SCHEMA, "tree": tree, "object_format": algorithm, "test_argv": command,
               "test_exit": exit_code, "status": status, "changed_tracked_inputs": changed,
               "snapshot": snapshot.as_posix(), "tracked_files": len(entries)}
    if probe is not None:
        probe_changed = not unchanged(probe_folder, probe_path, probe["mode"], probe["blob"], algorithm)
        if probe_changed and status != "timed_out":
            receipt["status"] = "probe_changed"
        receipt.update(probe=probe, probe_changed=probe_changed)
    with args.receipt.open("x", encoding="utf-8") as output:
        json.dump(receipt, output, indent=2)
        output.write("\n")
    return receipt, int(receipt["status"] != "passed")


def check_match(args):
    receipt = json.loads(args.receipt.read_text(encoding="utf-8-sig"))
    if not isinstance(receipt, dict) or receipt.get("schema") != SCHEMA:
        raise ValueError("Expected a tree-check receipt")
    if args.index:
        # Git may set GIT_INDEX_FILE to a temporary proposal for this hook invocation.
        tree = git(args.repo, "write-tree").decode().strip()
        target = {"proposed_tree": tree}
    else:
        commit = git(args.repo, "rev-parse", "--verify", "--end-of-options", args.commit + "^{commit}").decode().strip()
        tree = git(args.repo, "rev-parse", commit + "^{tree}").decode().strip()
        target = {"commit": commit, "committed_tree": tree}
    algorithm = git(args.repo, "rev-parse", "--show-object-format").decode().strip()
    same_tree = receipt.get("object_format") == algorithm and receipt.get("tree") == tree
    exit_code = receipt.get("test_exit")
    passed = (receipt.get("status") == "passed" and type(exit_code) is int and exit_code == 0
              and receipt.get("changed_tracked_inputs") == [])
    matched = same_tree and passed
    if args.probe:
        expected_probe = probe_binding(args.repo, args.probe, algorithm)
        same_probe = receipt.get("probe") == expected_probe and receipt.get("probe_changed") is False
        target["same_probe"] = same_probe
        matched = matched and same_probe
    if args.composition:
        base, proposal, onto = args.composition
        onto = exact_oid(args.repo, onto, algorithm, "commit")
        onto_tree = git(args.repo, "rev-parse", onto + "^{tree}").decode().strip()
        composition, status = compose_trees(args.repo, base, onto_tree, proposal, algorithm)
        headers = git(args.repo, "cat-file", "commit", commit).split(b"\n\n", 1)[0]
        first_parent = next((line[7:].decode() for line in headers.splitlines() if line.startswith(b"parent ")), None)
        same_parent = first_parent == onto
        same_composed_tree = status == 0 and composition["tree"] == tree
        target["composition"] = {
            **composition, "onto_commit": onto, "first_parent": first_parent,
            "same_parent": same_parent, "same_composed_tree": same_composed_tree,
        }
        matched = matched and same_parent and same_composed_tree
    return {**target, "tested_tree": receipt.get("tree"),
            "same_tree": same_tree, "passed_receipt": passed, "matched": matched}, int(not matched)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    actions = parser.add_subparsers(dest="action", required=True)
    run = actions.add_parser("run", help="Check the index tree or a supplied tree in its own directory.")
    run.add_argument("--repo", type=Path, default=Path.cwd())
    run.add_argument("--tree", help="Full tree object ID. When omitted, capture the current index.")
    run.add_argument("--probe", nargs=2, metavar=("TREE", "PATH"), help="Run a Python probe from a pinned tree.")
    run.add_argument("--scratch", type=Path, required=True)
    run.add_argument("--receipt", type=Path, required=True)
    run.add_argument("--timeout", type=int, default=60)
    run.add_argument("command", nargs=argparse.REMAINDER)
    compose = actions.add_parser("compose", help="Combine two recorded proposals from a common base tree.")
    compose.add_argument("--repo", type=Path, default=Path.cwd())
    compose.add_argument("--base", required=True, help="Full object ID of the starting tree.")
    compose.add_argument("left", help="Full object ID of the first proposal tree.")
    compose.add_argument("right", help="Full object ID of the second proposal tree.")
    match = actions.add_parser("match", help="Check a passing receipt against a commit or the active index.")
    match.add_argument("--repo", type=Path, default=Path.cwd())
    match.add_argument("--index", action="store_true", help="Compare the effective Git index visible to this process.")
    match.add_argument("--probe", nargs=2, metavar=("TREE", "PATH"), help="Require the same pinned probe.")
    match.add_argument("--composition", nargs=3, metavar=("BASE_TREE", "PROPOSAL_TREE", "ONTO_COMMIT"),
                       help="Require a commit to replay the recorded proposal onto the exact target commit.")
    match.add_argument("receipt", type=Path)
    match.add_argument("commit", nargs="?")
    args = parser.parse_args()
    if args.action == "match" and args.index == (args.commit is not None):
        parser.error("match requires either --index or a commit")
    if args.action == "match" and args.index and args.composition:
        parser.error("match --composition requires a commit with a first parent")
    handlers = {"run": check_run, "compose": check_compose, "match": check_match}
    try:
        report, status = handlers[args.action](args)
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        parser.exit(2, f"{error}\n")
    print(json.dumps(report, ensure_ascii=False))
    return status


if __name__ == "__main__":
    raise SystemExit(main())

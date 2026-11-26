"""Run a check on a staged Git tree and match the receipt before or after a commit."""

import argparse
import hashlib
import json
import os
from pathlib import Path, PureWindowsPath
import stat
import subprocess
import tempfile


SCHEMA = "nexum-tree-check-v1"
BLOCKED_NAMES = {".git", ".env", "auth.json", "config.json"}
BLOB_CHUNK_BYTES = 1024 * 1024
is_reserved_path = getattr(os.path, "isreserved", lambda name: PureWindowsPath(name).is_reserved())


def git(repo, *args):
    result = subprocess.run(["git", "-C", str(repo), *args], capture_output=True, timeout=30)
    if result.returncode:
        raise ValueError(f"git {args[0]}: {result.stderr.decode(errors='replace').strip()}")
    return result.stdout


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
    command = ["git", "-C", str(repo), "cat-file", "--batch"]
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


def check_run(args):
    command = args.command[1:] if args.command[:1] == ["--"] else args.command
    if not command or args.timeout < 1:
        raise ValueError("Pass a positive timeout and an explicit check command after --")
    if args.receipt.exists():
        raise ValueError(f"Receipt already exists: {args.receipt}")

    tree = git(args.repo, "write-tree").decode().strip()
    algorithm = git(args.repo, "rev-parse", "--show-object-format").decode().strip()
    entries = read_entries(args.repo, tree)
    snapshot = Path(tempfile.mkdtemp(prefix="nexum-tree-", dir=args.scratch.resolve(strict=True)))
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
    with args.receipt.open("x", encoding="utf-8") as output:
        json.dump(receipt, output, indent=2)
        output.write("\n")
    return receipt, int(status != "passed")


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
    return {**target, "tested_tree": receipt.get("tree"),
            "same_tree": same_tree, "passed_receipt": passed, "matched": matched}, int(not matched)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    actions = parser.add_subparsers(dest="action", required=True)
    run = actions.add_parser("run", help="Capture the index and run a check in its own directory.")
    run.add_argument("--repo", type=Path, default=Path.cwd())
    run.add_argument("--scratch", type=Path, required=True)
    run.add_argument("--receipt", type=Path, required=True)
    run.add_argument("--timeout", type=int, default=60)
    run.add_argument("command", nargs=argparse.REMAINDER)
    match = actions.add_parser("match", help="Check a passing receipt against a commit or the active index.")
    match.add_argument("--repo", type=Path, default=Path.cwd())
    match.add_argument("--index", action="store_true", help="Compare the effective Git index visible to this process.")
    match.add_argument("receipt", type=Path)
    match.add_argument("commit", nargs="?")
    args = parser.parse_args()
    if args.action == "match" and args.index == (args.commit is not None):
        parser.error("match requires either --index or a commit")
    try:
        report, status = (check_run if args.action == "run" else check_match)(args)
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        parser.exit(2, f"{error}\n")
    print(json.dumps(report, ensure_ascii=False))
    return status


if __name__ == "__main__":
    raise SystemExit(main())

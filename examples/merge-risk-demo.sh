#!/usr/bin/env sh
# Build a throwaway repo with two branches that git merges cleanly but that
# break each other. Check Nexum's verdict against Python on both merged trees.
#
#   sh examples/merge-risk-demo.sh            # repo goes in a new temp dir
#   sh examples/merge-risk-demo.sh ./demo     # or in a directory you name
#
# Needs Git, Nexum and Python 3. Set NEX or PYTHON to select a binary.
set -eu

resolve_executable() {
    case "$1" in
        /*|[A-Za-z]:/*) printf '%s\n' "$1" ;;
        */*) printf '%s/%s\n' "$PWD" "$1" ;;
        *) printf '%s\n' "$1" ;;
    esac
}

NEX=$(resolve_executable "${NEX:-nex}")
PYTHON=$(resolve_executable "${PYTHON:-python3}")
DIR="${1:-$(mktemp -d 2>/dev/null || mktemp -d -t nexdemo)}"
mkdir -p "$DIR"
cd "$DIR"
if [ -n "$(ls -A)" ]; then
    echo 'Choose an empty directory for the demo repository.' >&2
    exit 1
fi

git init -q -b main --template=
git config user.email demo@example.com
git config user.name "nex demo"
git config core.autocrlf false
git config core.hooksPath .git/hooks
git config commit.gpgsign false

cat > lib.py <<'EOF'
def compute_total(items):
    return sum(items)


def format_name(first, last):
    return f"{first} {last}"
EOF
cat > app.py <<'EOF'
from lib import compute_total


def main():
    print(compute_total([1, 2, 3]))
EOF
git add lib.py app.py
git commit -q -m "base"

# feature/a: change a signature and rename a function.
git checkout -q -b feature/a
cat > lib.py <<'EOF'
def compute_total(items, tax_rate):
    return sum(items) * (1 + tax_rate)


def full_name(first, last):
    return f"{first} {last}"
EOF
git commit -q -am "a: add tax_rate, rename format_name -> full_name"

# feature/b: new code that calls the old signature and the old name.
git checkout -q main
git checkout -q -b feature/b
cat > report.py <<'EOF'
from lib import compute_total, format_name


def report():
    return format_name("Ada", "Lovelace"), compute_total([4, 5])
EOF
git add report.py
git commit -q -m "b: add report using compute_total and format_name"

# feature/c: an unrelated change (the control).
git checkout -q main
git checkout -q -b feature/c
cat >> app.py <<'EOF'


def banner():
    return "hello"
EOF
git commit -q -am "c: add banner"
git checkout -q main

echo "Demo repo: $DIR"

run_check() {
    case_name=$1
    branch_a=$2
    branch_b=$3
    report=".git/demo-$case_name.json"
    status=0
    "$NEX" check "$branch_a" "$branch_b" --format json > "$report" || status=$?
    "$PYTHON" -E -s -B - "$case_name" "$branch_a" "$branch_b" "$status" "$report" <<'PY'
import json
from pathlib import Path
import sys

case_name, branch_a, branch_b, actual_exit, report_path = sys.argv[1:]
expected_exit = 1 if case_name == "broken" else 0
if int(actual_exit) != expected_exit:
    raise SystemExit(f"{case_name}: nex exited {actual_exit}; expected {expected_exit}")

try:
    report = json.loads(Path(report_path).read_text(encoding="utf-8"))
except (OSError, ValueError):
    raise SystemExit(f"{case_name}: no readable Nexum JSON report at {report_path}") from None
if not isinstance(report, dict) or (report.get("branch_a"), report.get("branch_b")) != (branch_a, branch_b):
    raise SystemExit(f"{case_name}: report describes a different branch pair")

conflicts = report.get("conflicts")
if not isinstance(conflicts, list):
    raise SystemExit(f"{case_name}: report has no conflicts array")
if case_name == "broken":
    blocking = {
        (kind, entry["unit_a"]["name"], entry["unit_b"]["file_path"])
        for entry in conflicts if entry["severity"] == "Error"
        for kind in entry["kind"]
    }
    required = {
        ("SignatureMismatch", "compute_total", "report.py"),
        ("DeletedDependency", "format_name", "report.py"),
    }
    if not required <= blocking:
        raise SystemExit(f"{case_name}: missing blocking findings for report.py")
elif conflicts:
    raise SystemExit(f"{case_name}: nex returned conflicts for the working control")

print(f"{case_name}: nex exit {actual_exit}, {len(conflicts)} finding(s), report {report_path}")
PY
}

checks_passed=1
run_check broken feature/a feature/b || checks_passed=0
run_check control feature/b feature/c || checks_passed=0

git checkout -q -b proof/broken feature/a
git merge -q --no-edit --no-ff feature/b
"$PYTHON" -E -s -B - <<'PY'
from lib import compute_total

try:
    import report
except ImportError as error:
    if error.name != "lib" or "format_name" not in str(error):
        raise
else:
    raise SystemExit("broken: merged report unexpectedly imported")

try:
    compute_total([4, 5])
except TypeError as error:
    if "tax_rate" not in str(error):
        raise
else:
    raise SystemExit("broken: the old call unexpectedly worked")

print("broken: Git merged; Python reproduced the removed import and stale call")
PY

git checkout -q -b proof/control feature/b
git merge -q --no-edit --no-ff feature/c
"$PYTHON" -E -s -B - <<'PY'
from app import banner
from report import report

if (report(), banner()) != (("Ada Lovelace", 9), "hello"):
    raise SystemExit("control: merged program produced the wrong result")
print("control: Git merged; Python returned the expected report and banner")
PY
git checkout -q main

[ "$checks_passed" -eq 1 ]

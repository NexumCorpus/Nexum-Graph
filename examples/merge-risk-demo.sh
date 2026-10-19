#!/usr/bin/env sh
# Build a throwaway repo with two branches that git merges cleanly but that
# break each other, then run `nex check` on them (and on a harmless control).
#
#   sh examples/merge-risk-demo.sh            # repo goes in a new temp dir
#   sh examples/merge-risk-demo.sh ./demo     # or in a directory you name
#
# Needs `git` and `nex` on PATH (set NEX=/path/to/nex to use another binary).
set -eu

NEX="${NEX:-nex}"
DIR="${1:-$(mktemp -d 2>/dev/null || mktemp -d -t nexdemo)}"
mkdir -p "$DIR"
cd "$DIR"

git init -q -b main
git config user.email demo@example.com
git config user.name "nex demo"
git config core.autocrlf false

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
echo
echo "== git merge-tree feature/a feature/b (does git see a conflict?)"
if git merge-tree --write-tree feature/a feature/b >/dev/null 2>&1; then
    echo "git: merges cleanly"
else
    echo "git: textual conflict"
fi
echo
echo "== nex check feature/a feature/b (expected: exit 1, 3 errors)"
set +e
"$NEX" check feature/a feature/b
echo "exit $?"
echo
echo "== nex check feature/b feature/c (control; expected: exit 0)"
"$NEX" check feature/b feature/c
echo "exit $?"

#!/usr/bin/env python3
"""Install or check repo-managed Codex skills for Nexum Graph."""

from __future__ import annotations

import argparse
import filecmp
import json
import os
import shutil
import sys
from dataclasses import dataclass
from pathlib import Path

IGNORE_NAMES = {"__pycache__"}
IGNORE_SUFFIXES = {".pyc"}


@dataclass(frozen=True)
class SkillStatus:
    name: str
    source: str
    destination: str
    status: str
    detail: str


def repo_root() -> Path:
    return Path(__file__).resolve().parent.parent


def source_root() -> Path:
    return repo_root() / "codex-skills"


def codex_home() -> Path:
    return Path(os.environ.get("CODEX_HOME", str(Path.home() / ".codex")))


def destination_root() -> Path:
    return codex_home() / "skills"


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Install or check repo-managed Codex skills from codex-skills/."
    )
    parser.add_argument(
        "--skill",
        dest="skills",
        action="append",
        default=[],
        help="Specific skill name to sync or check. Repeatable.",
    )
    parser.add_argument(
        "--check",
        action="store_true",
        help="Check for local drift without writing files.",
    )
    parser.add_argument(
        "--list",
        action="store_true",
        help="List repo-managed skill names and exit.",
    )
    parser.add_argument(
        "--json",
        action="store_true",
        help="Emit machine-readable JSON instead of text.",
    )
    parser.add_argument(
        "--source-root",
        type=Path,
        default=None,
        help="Override the repo skill source directory. Intended for tests.",
    )
    parser.add_argument(
        "--dest-root",
        type=Path,
        default=None,
        help="Override the install destination root. Intended for tests.",
    )
    return parser.parse_args()


def list_skill_names(root: Path | None = None) -> list[str]:
    skills_root = (root or source_root()).resolve()
    if not skills_root.exists():
        return []

    names: list[str] = []
    for child in sorted(skills_root.iterdir()):
        if child.is_dir() and (child / "SKILL.md").exists():
            names.append(child.name)
    return names


def should_ignore(path: Path) -> bool:
    return any(part in IGNORE_NAMES for part in path.parts) or path.suffix.lower() in IGNORE_SUFFIXES


def collect_relative_files(root: Path) -> list[Path]:
    if not root.exists():
        return []

    files: list[Path] = []
    for path in root.rglob("*"):
        if not path.is_file():
            continue
        relative = path.relative_to(root)
        if should_ignore(relative):
            continue
        files.append(relative)
    return sorted(files)


def compare_skill_dirs(source: Path, destination: Path) -> tuple[str, str]:
    if not source.exists():
        return "missing_source", "source skill is missing"
    if not destination.exists():
        return "missing_destination", "destination skill is not installed"

    source_files = collect_relative_files(source)
    dest_files = collect_relative_files(destination)
    if source_files != dest_files:
        missing = sorted(set(source_files) - set(dest_files))
        extra = sorted(set(dest_files) - set(source_files))
        detail_parts: list[str] = []
        if missing:
            detail_parts.append("missing: " + ", ".join(str(path) for path in missing))
        if extra:
            detail_parts.append("extra: " + ", ".join(str(path) for path in extra))
        return "out_of_sync", "; ".join(detail_parts)

    for relative in source_files:
        if not filecmp.cmp(source / relative, destination / relative, shallow=False):
            return "out_of_sync", f"content differs: {relative}"

    return "in_sync", "installed copy matches repo"


def sync_skill(source: Path, destination: Path) -> None:
    if destination.exists():
        shutil.rmtree(destination)
    destination.parent.mkdir(parents=True, exist_ok=True)
    shutil.copytree(source, destination, ignore=shutil.ignore_patterns(*IGNORE_NAMES, "*.pyc"))


def evaluate_skill(skill_name: str, sources: Path, destinations: Path, check_only: bool) -> SkillStatus:
    source = sources / skill_name
    destination = destinations / skill_name
    current_status, detail = compare_skill_dirs(source, destination)

    if check_only or current_status == "missing_source":
        return SkillStatus(
            name=skill_name,
            source=str(source),
            destination=str(destination),
            status=current_status,
            detail=detail,
        )

    if current_status != "in_sync":
        sync_skill(source, destination)
        current_status, detail = compare_skill_dirs(source, destination)

    return SkillStatus(
        name=skill_name,
        source=str(source),
        destination=str(destination),
        status=current_status,
        detail=detail,
    )


def print_text(statuses: list[SkillStatus]) -> None:
    for status in statuses:
        print(f"[{status.status.upper()}] {status.name}")
        print(f"  source: {status.source}")
        print(f"  destination: {status.destination}")
        print(f"  detail: {status.detail}")


def print_json(statuses: list[SkillStatus]) -> None:
    print(
        json.dumps(
            [
                {
                    "name": status.name,
                    "source": status.source,
                    "destination": status.destination,
                    "status": status.status,
                    "detail": status.detail,
                }
                for status in statuses
            ],
            indent=2,
        )
    )


def main() -> int:
    args = parse_args()
    sources = (args.source_root or source_root()).resolve()
    destinations = (args.dest_root or destination_root()).resolve()

    available_skills = list_skill_names(sources)
    if args.list:
        for skill_name in available_skills:
            print(skill_name)
        return 0

    selected = args.skills or available_skills
    unknown = sorted(set(selected) - set(available_skills))
    if unknown:
        print(f"Unknown skill(s): {', '.join(unknown)}", file=sys.stderr)
        return 2

    statuses = [
        evaluate_skill(skill_name, sources=sources, destinations=destinations, check_only=args.check)
        for skill_name in selected
    ]

    if args.json:
        print_json(statuses)
    else:
        print_text(statuses)

    if any(status.status == "missing_source" for status in statuses):
        return 1
    if args.check and any(status.status != "in_sync" for status in statuses):
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

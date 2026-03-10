#!/usr/bin/env python3
"""Inspect Nexum Graph public GitHub state and release workflow progress."""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
import time
import urllib.parse
import urllib.request
from dataclasses import asdict, dataclass
from pathlib import Path

API_BASE = "https://api.github.com"
DEFAULT_REPO = "NexumCorpus/Nexum-Graph"
DEFAULT_WORKFLOW = "Release"
DEFAULT_USER_AGENT = "NexumGraphReleaseWatch/1.0"


class GitHubWatchError(RuntimeError):
    """Raised when GitHub state could not be fetched or interpreted."""


@dataclass(frozen=True)
class PublicSummary:
    repo: str
    default_branch: str
    stars: int
    forks: int
    watchers: int
    open_issues: int
    latest_release_tag: str | None
    latest_release_published_at: str | None
    latest_release_assets: list[str]
    latest_release_run_id: int | None
    latest_release_run_status: str | None
    latest_release_run_conclusion: str | None
    latest_release_run_url: str | None


@dataclass(frozen=True)
class JobSummary:
    name: str
    status: str
    conclusion: str | None
    html_url: str | None


@dataclass(frozen=True)
class ReleaseStatus:
    repo: str
    tag: str
    workflow_name: str
    release_exists: bool
    release_url: str | None
    published_at: str | None
    asset_names: list[str]
    workflow_run_id: int | None
    workflow_run_status: str | None
    workflow_run_conclusion: str | None
    workflow_run_url: str | None
    jobs: list[JobSummary]

    @property
    def ready(self) -> bool:
        return self.release_exists and self.workflow_run_conclusion == "success"


def repo_root() -> Path:
    return Path(__file__).resolve().parent.parent


def parse_repo_slug_from_remote(remote_url: str) -> str | None:
    normalized = remote_url.strip()
    if normalized.endswith(".git"):
        normalized = normalized[:-4]
    if normalized.startswith("git@github.com:"):
        return normalized.removeprefix("git@github.com:")
    if normalized.startswith("https://github.com/"):
        return normalized.removeprefix("https://github.com/")
    if normalized.startswith("http://github.com/"):
        return normalized.removeprefix("http://github.com/")
    return None


def detect_repo_slug(root: Path | None = None) -> str:
    root = root or repo_root()
    try:
        completed = subprocess.run(
            ["git", "remote", "get-url", "origin"],
            cwd=root,
            capture_output=True,
            check=True,
            text=True,
        )
    except (OSError, subprocess.CalledProcessError):
        return DEFAULT_REPO

    slug = parse_repo_slug_from_remote(completed.stdout.strip())
    return slug or DEFAULT_REPO


def fetch_json(path: str, params: dict[str, str | int] | None = None) -> object:
    query = ""
    if params:
        query = "?" + urllib.parse.urlencode(params)
    request = urllib.request.Request(
        f"{API_BASE}{path}{query}",
        headers={
            "Accept": "application/vnd.github+json",
            "User-Agent": DEFAULT_USER_AGENT,
        },
    )
    with urllib.request.urlopen(request, timeout=30) as response:
        return json.load(response)


def fetch_url_json(url: str) -> object:
    request = urllib.request.Request(
        url,
        headers={
            "Accept": "application/vnd.github+json",
            "User-Agent": DEFAULT_USER_AGENT,
        },
    )
    with urllib.request.urlopen(request, timeout=30) as response:
        return json.load(response)


def fetch_repo_payload(repo: str) -> dict:
    payload = fetch_json(f"/repos/{repo}")
    if not isinstance(payload, dict):
        raise GitHubWatchError(f"unexpected repo payload for {repo!r}")
    return payload


def fetch_releases(repo: str, limit: int = 10) -> list[dict]:
    payload = fetch_json(f"/repos/{repo}/releases", {"per_page": limit})
    if not isinstance(payload, list):
        raise GitHubWatchError(f"unexpected releases payload for {repo!r}")
    return payload


def fetch_workflow_runs(repo: str, limit: int = 20) -> list[dict]:
    payload = fetch_json(f"/repos/{repo}/actions/runs", {"per_page": limit})
    if not isinstance(payload, dict):
        raise GitHubWatchError(f"unexpected workflow payload for {repo!r}")
    runs = payload.get("workflow_runs", [])
    if not isinstance(runs, list):
        raise GitHubWatchError(f"unexpected workflow_runs payload for {repo!r}")
    return runs


def fetch_jobs(jobs_url: str) -> list[JobSummary]:
    payload = fetch_url_json(jobs_url)
    if not isinstance(payload, dict):
        raise GitHubWatchError("unexpected jobs payload")
    jobs = payload.get("jobs", [])
    if not isinstance(jobs, list):
        raise GitHubWatchError("unexpected jobs list")
    return [
        JobSummary(
            name=str(job.get("name", "")),
            status=str(job.get("status", "")),
            conclusion=job.get("conclusion"),
            html_url=job.get("html_url"),
        )
        for job in jobs
    ]


def latest_release_run(runs: list[dict], workflow_name: str) -> dict | None:
    for run in runs:
        if run.get("name") == workflow_name:
            return run
    return None


def release_run_for_tag(runs: list[dict], tag: str, workflow_name: str) -> dict | None:
    for run in runs:
        if run.get("name") != workflow_name:
            continue
        if run.get("head_branch") == tag:
            return run
    return None


def build_public_summary(
    repo: str,
    repo_payload: dict,
    releases: list[dict],
    runs: list[dict],
    workflow_name: str = DEFAULT_WORKFLOW,
) -> PublicSummary:
    latest_release = releases[0] if releases else None
    latest_run = latest_release_run(runs, workflow_name)
    assets = latest_release.get("assets", []) if isinstance(latest_release, dict) else []
    asset_names = [str(asset.get("name", "")) for asset in assets if isinstance(asset, dict)]
    return PublicSummary(
        repo=repo,
        default_branch=str(repo_payload.get("default_branch", "")),
        stars=int(repo_payload.get("stargazers_count", 0)),
        forks=int(repo_payload.get("forks_count", 0)),
        watchers=int(repo_payload.get("subscribers_count", 0)),
        open_issues=int(repo_payload.get("open_issues_count", 0)),
        latest_release_tag=latest_release.get("tag_name") if latest_release else None,
        latest_release_published_at=latest_release.get("published_at") if latest_release else None,
        latest_release_assets=asset_names,
        latest_release_run_id=latest_run.get("id") if latest_run else None,
        latest_release_run_status=latest_run.get("status") if latest_run else None,
        latest_release_run_conclusion=latest_run.get("conclusion") if latest_run else None,
        latest_release_run_url=latest_run.get("html_url") if latest_run else None,
    )


def build_release_status(
    repo: str,
    tag: str,
    releases: list[dict],
    runs: list[dict],
    workflow_name: str = DEFAULT_WORKFLOW,
) -> ReleaseStatus:
    release = next((item for item in releases if item.get("tag_name") == tag), None)
    run = release_run_for_tag(runs, tag, workflow_name)
    jobs = fetch_jobs(str(run["jobs_url"])) if run and run.get("jobs_url") else []
    asset_names = [
        str(asset.get("name", ""))
        for asset in (release.get("assets", []) if isinstance(release, dict) else [])
        if isinstance(asset, dict)
    ]
    return ReleaseStatus(
        repo=repo,
        tag=tag,
        workflow_name=workflow_name,
        release_exists=release is not None,
        release_url=release.get("html_url") if release else None,
        published_at=release.get("published_at") if release else None,
        asset_names=asset_names,
        workflow_run_id=run.get("id") if run else None,
        workflow_run_status=run.get("status") if run else None,
        workflow_run_conclusion=run.get("conclusion") if run else None,
        workflow_run_url=run.get("html_url") if run else None,
        jobs=jobs,
    )


def watch_release(
    repo: str,
    tag: str,
    workflow_name: str,
    wait_seconds: int,
    poll_interval: int,
) -> ReleaseStatus:
    deadline = time.time() + wait_seconds
    while True:
        status = build_release_status(
            repo=repo,
            tag=tag,
            releases=fetch_releases(repo),
            runs=fetch_workflow_runs(repo),
            workflow_name=workflow_name,
        )
        if wait_seconds <= 0:
            return status
        if status.ready:
            return status
        if status.workflow_run_conclusion and status.workflow_run_conclusion != "success":
            return status
        if time.time() >= deadline:
            return status
        time.sleep(poll_interval)


def render_public_summary_text(summary: PublicSummary) -> str:
    lines = [
        "GitHub Public Summary",
        "====================",
        f"Repo: {summary.repo}",
        f"Default branch: {summary.default_branch}",
        f"Stars: {summary.stars}",
        f"Forks: {summary.forks}",
        f"Watchers: {summary.watchers}",
        f"Open issues: {summary.open_issues}",
    ]
    if summary.latest_release_tag:
        lines.extend(
            [
                f"Latest release: {summary.latest_release_tag}",
                f"Published at: {summary.latest_release_published_at}",
                f"Assets: {', '.join(summary.latest_release_assets) or '(none)'}",
            ]
        )
    else:
        lines.append("Latest release: (none)")
    if summary.latest_release_run_id:
        lines.extend(
            [
                f"Latest {DEFAULT_WORKFLOW} run: {summary.latest_release_run_id}",
                (
                    "Workflow status: "
                    f"{summary.latest_release_run_status}"
                    + (
                        f" ({summary.latest_release_run_conclusion})"
                        if summary.latest_release_run_conclusion
                        else ""
                    )
                ),
                f"Workflow URL: {summary.latest_release_run_url}",
            ]
        )
    return "\n".join(lines)


def render_release_status_text(status: ReleaseStatus) -> str:
    lines = [
        "GitHub Release Status",
        "=====================",
        f"Repo: {status.repo}",
        f"Tag: {status.tag}",
        f"Release object: {'published' if status.release_exists else 'missing'}",
    ]
    if status.release_exists:
        lines.extend(
            [
                f"Release URL: {status.release_url}",
                f"Published at: {status.published_at}",
                f"Assets: {', '.join(status.asset_names) or '(none)'}",
            ]
        )
    if status.workflow_run_id:
        workflow_line = f"Workflow run: {status.workflow_run_id} ({status.workflow_run_status}"
        if status.workflow_run_conclusion:
            workflow_line += f", {status.workflow_run_conclusion}"
        workflow_line += ")"
        lines.extend([workflow_line, f"Workflow URL: {status.workflow_run_url}"])
    else:
        lines.append("Workflow run: not found")
    if status.jobs:
        lines.append("Jobs:")
        for job in status.jobs:
            job_line = f"  - {job.name}: {job.status}"
            if job.conclusion:
                job_line += f" ({job.conclusion})"
            lines.append(job_line)
    lines.append(f"Ready: {'yes' if status.ready else 'no'}")
    return "\n".join(lines)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--repo",
        default=None,
        help="GitHub repo in owner/name form. Defaults to origin remote or NexumCorpus/Nexum-Graph.",
    )
    parser.add_argument(
        "--json",
        action="store_true",
        help="Emit machine-readable JSON.",
    )
    subparsers = parser.add_subparsers(dest="command", required=True)

    subparsers.add_parser("public-summary", help="Show repo-level public state and latest release run.")

    release_status = subparsers.add_parser(
        "release-status",
        help="Show or watch release workflow state for a tag.",
    )
    release_status.add_argument("--tag", required=True, help="Release tag, e.g. v0.1.0.")
    release_status.add_argument(
        "--workflow-name",
        default=DEFAULT_WORKFLOW,
        help=f"Workflow name to inspect. Defaults to {DEFAULT_WORKFLOW!r}.",
    )
    release_status.add_argument(
        "--wait-seconds",
        type=int,
        default=0,
        help="Poll until release appears or workflow completes, up to this many seconds.",
    )
    release_status.add_argument(
        "--poll-interval",
        type=int,
        default=30,
        help="Polling interval when waiting. Defaults to 30 seconds.",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    repo = args.repo or detect_repo_slug()

    if args.command == "public-summary":
        summary = build_public_summary(
            repo=repo,
            repo_payload=fetch_repo_payload(repo),
            releases=fetch_releases(repo),
            runs=fetch_workflow_runs(repo),
        )
        if args.json:
            print(json.dumps(asdict(summary), indent=2))
        else:
            print(render_public_summary_text(summary))
        return 0

    if args.command == "release-status":
        status = watch_release(
            repo=repo,
            tag=args.tag,
            workflow_name=args.workflow_name,
            wait_seconds=max(args.wait_seconds, 0),
            poll_interval=max(args.poll_interval, 1),
        )
        if args.json:
            print(json.dumps(asdict(status), indent=2))
        else:
            print(render_release_status_text(status))
        if status.workflow_run_conclusion and status.workflow_run_conclusion != "success":
            return 1
        if args.wait_seconds > 0 and not status.ready:
            return 1
        return 0

    raise GitHubWatchError(f"unsupported command: {args.command}")


if __name__ == "__main__":
    raise SystemExit(main())

from __future__ import annotations

import sys
import unittest
from pathlib import Path
from unittest import mock

TOOLS_DIR = Path(__file__).resolve().parent
if str(TOOLS_DIR) not in sys.path:
    sys.path.insert(0, str(TOOLS_DIR))

import github_release_watch


class GitHubReleaseWatchTests(unittest.TestCase):
    def test_parse_repo_slug_from_remote_supports_https_and_ssh(self) -> None:
        self.assertEqual(
            github_release_watch.parse_repo_slug_from_remote(
                "https://github.com/NexumCorpus/Nexum-Graph.git"
            ),
            "NexumCorpus/Nexum-Graph",
        )
        self.assertEqual(
            github_release_watch.parse_repo_slug_from_remote(
                "git@github.com:NexumCorpus/Nexum-Graph.git"
            ),
            "NexumCorpus/Nexum-Graph",
        )
        self.assertIsNone(
            github_release_watch.parse_repo_slug_from_remote("https://example.com/not-github")
        )

    def test_build_public_summary_includes_latest_release_and_run(self) -> None:
        summary = github_release_watch.build_public_summary(
            repo="NexumCorpus/Nexum-Graph",
            repo_payload={
                "default_branch": "main",
                "stargazers_count": 12,
                "forks_count": 3,
                "subscribers_count": 2,
                "open_issues_count": 5,
            },
            releases=[
                {
                    "tag_name": "v0.1.0",
                    "published_at": "2026-03-10T19:04:29Z",
                    "assets": [{"name": "nexum-graph-v0.1.0-x86_64-unknown-linux-gnu.tar.gz"}],
                }
            ],
            runs=[
                {
                    "id": 42,
                    "name": "Release",
                    "status": "completed",
                    "conclusion": "success",
                    "html_url": "https://github.com/example/run/42",
                }
            ],
        )

        self.assertEqual(summary.latest_release_tag, "v0.1.0")
        self.assertEqual(summary.latest_release_assets, ["nexum-graph-v0.1.0-x86_64-unknown-linux-gnu.tar.gz"])
        self.assertEqual(summary.latest_release_run_id, 42)
        self.assertEqual(summary.latest_release_run_conclusion, "success")

    def test_build_release_status_matches_tagged_workflow_run(self) -> None:
        with mock.patch.object(
            github_release_watch,
            "fetch_jobs",
            return_value=[github_release_watch.JobSummary("Build VS Code extension", "completed", "success", None)],
        ):
            status = github_release_watch.build_release_status(
                repo="NexumCorpus/Nexum-Graph",
                tag="v0.1.0",
                workflow_name="Release",
                releases=[
                    {
                        "tag_name": "v0.1.0",
                        "html_url": "https://github.com/NexumCorpus/Nexum-Graph/releases/tag/v0.1.0",
                        "published_at": "2026-03-10T19:04:29Z",
                        "assets": [{"name": "SHA256SUMS.txt"}],
                    }
                ],
                runs=[
                    {
                        "id": 9001,
                        "name": "Release",
                        "head_branch": "v0.1.0",
                        "status": "completed",
                        "conclusion": "success",
                        "html_url": "https://github.com/NexumCorpus/Nexum-Graph/actions/runs/9001",
                        "jobs_url": "https://api.github.com/jobs/9001",
                    }
                ],
            )

        self.assertTrue(status.release_exists)
        self.assertEqual(status.workflow_run_id, 9001)
        self.assertEqual(status.asset_names, ["SHA256SUMS.txt"])
        self.assertTrue(status.ready)
        self.assertEqual(len(status.jobs), 1)

    def test_watch_release_returns_success_once_release_is_ready(self) -> None:
        pending = github_release_watch.ReleaseStatus(
            repo="NexumCorpus/Nexum-Graph",
            tag="v0.1.0",
            workflow_name="Release",
            release_exists=False,
            release_url=None,
            published_at=None,
            asset_names=[],
            workflow_run_id=1,
            workflow_run_status="in_progress",
            workflow_run_conclusion=None,
            workflow_run_url="https://github.com/example/run/1",
            jobs=[],
        )
        ready = github_release_watch.ReleaseStatus(
            repo="NexumCorpus/Nexum-Graph",
            tag="v0.1.0",
            workflow_name="Release",
            release_exists=True,
            release_url="https://github.com/NexumCorpus/Nexum-Graph/releases/tag/v0.1.0",
            published_at="2026-03-10T19:04:29Z",
            asset_names=["SHA256SUMS.txt"],
            workflow_run_id=1,
            workflow_run_status="completed",
            workflow_run_conclusion="success",
            workflow_run_url="https://github.com/example/run/1",
            jobs=[],
        )

        with (
            mock.patch.object(
                github_release_watch,
                "build_release_status",
                side_effect=[pending, ready],
            ),
            mock.patch.object(github_release_watch.time, "sleep"),
        ):
            result = github_release_watch.watch_release(
                repo="NexumCorpus/Nexum-Graph",
                tag="v0.1.0",
                workflow_name="Release",
                wait_seconds=60,
                poll_interval=1,
            )

        self.assertTrue(result.ready)
        self.assertEqual(result.workflow_run_conclusion, "success")


if __name__ == "__main__":
    unittest.main()

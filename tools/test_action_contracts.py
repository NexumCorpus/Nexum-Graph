import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parent.parent


class ActionContractTests(unittest.TestCase):
    def test_action_exposes_advisory_gate_contract(self) -> None:
        content = (REPO_ROOT / "action.yml").read_text(encoding="utf-8")

        self.assertIn("gate-mode:", content)
        self.assertIn("strict, errors-only, or advisory", content)
        self.assertIn("gate-exit-code:", content)
        self.assertIn("gate-status:", content)
        self.assertIn("exit \"$gate_exit_code\"", content)

    def test_reusable_workflow_exists_for_one_line_adoption(self) -> None:
        content = (REPO_ROOT / ".github" / "workflows" / "reusable-semantic-check.yml").read_text(
            encoding="utf-8"
        )

        self.assertIn("on:\n  workflow_call:", content)
        self.assertIn("uses: ./.nexum-graph-action", content)
        self.assertIn("post-pr-comment:", content)
        self.assertIn("upload-sarif:", content)

    def test_readme_documents_gate_modes(self) -> None:
        content = (REPO_ROOT / "README.md").read_text(encoding="utf-8")

        self.assertIn(
            "uses: NexumCorpus/Nexum-Graph/.github/workflows/reusable-semantic-check.yml@v0.1.0",
            content,
        )
        self.assertIn("gate-mode: errors-only", content)
        self.assertIn("`strict`: block on errors and warnings", content)
        self.assertIn("`errors-only`: block on errors, keep warnings advisory", content)
        self.assertIn("`advisory`: never fail the action for semantic conflicts", content)
        self.assertIn("[docs/github-rollout.md](./docs/github-rollout.md)", content)

    def test_rollout_doc_covers_all_three_modes(self) -> None:
        content = (REPO_ROOT / "docs" / "github-rollout.md").read_text(encoding="utf-8")

        self.assertIn("gate-mode: advisory", content)
        self.assertIn("gate-mode: errors-only", content)
        self.assertIn("gate-mode: strict", content)
        self.assertIn("post-pr-comment: false", content)
        self.assertIn("upload-sarif: false", content)


if __name__ == "__main__":
    unittest.main()

from __future__ import annotations

import sys
import tempfile
import unittest
from pathlib import Path

TOOLS_DIR = Path(__file__).resolve().parent
if str(TOOLS_DIR) not in sys.path:
    sys.path.insert(0, str(TOOLS_DIR))

import sync_nex_skills


def create_skill(root: Path, name: str, body: str = "# Skill\n") -> Path:
    skill_dir = root / name
    (skill_dir / "agents").mkdir(parents=True)
    (skill_dir / "references").mkdir()
    (skill_dir / "SKILL.md").write_text(body, encoding="utf-8")
    (skill_dir / "agents" / "openai.yaml").write_text("interface:\n  display_name: test\n", encoding="utf-8")
    (skill_dir / "references" / "note.md").write_text("note\n", encoding="utf-8")
    return skill_dir


class SkillSyncTests(unittest.TestCase):
    def test_list_skill_names_only_includes_skill_dirs(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            create_skill(root, "nexum-graph-sprint")
            (root / "not-a-skill").mkdir()
            self.assertEqual(sync_nex_skills.list_skill_names(root), ["nexum-graph-sprint"])

    def test_compare_skill_dirs_detects_missing_destination(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            source = create_skill(root / "source", "nexum-graph-sprint")
            status, detail = sync_nex_skills.compare_skill_dirs(source, root / "dest" / "nexum-graph-sprint")
            self.assertEqual(status, "missing_destination")
            self.assertIn("not installed", detail)

    def test_sync_skill_copies_repo_skill_tree(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            source = create_skill(root / "source", "nexum-graph-sprint")
            destination = root / "dest" / "nexum-graph-sprint"
            sync_nex_skills.sync_skill(source, destination)
            status, detail = sync_nex_skills.compare_skill_dirs(source, destination)
            self.assertEqual(status, "in_sync")
            self.assertEqual(detail, "installed copy matches repo")

    def test_compare_skill_dirs_detects_content_drift(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            source = create_skill(root / "source", "nexum-graph-sprint")
            destination = create_skill(root / "dest", "nexum-graph-sprint")
            (destination / "references" / "note.md").write_text("changed\n", encoding="utf-8")
            status, detail = sync_nex_skills.compare_skill_dirs(source, destination)
            self.assertEqual(status, "out_of_sync")
            self.assertIn("content differs", detail)

    def test_evaluate_skill_syncs_when_not_in_check_mode(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            sources = root / "source"
            destinations = root / "dest"
            create_skill(sources, "nexum-graph-sprint")

            status = sync_nex_skills.evaluate_skill(
                "nexum-graph-sprint",
                sources=sources,
                destinations=destinations,
                check_only=False,
            )
            self.assertEqual(status.status, "in_sync")
            self.assertTrue((destinations / "nexum-graph-sprint" / "SKILL.md").exists())

    def test_evaluate_skill_reports_drift_in_check_mode(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            sources = root / "source"
            destinations = root / "dest"
            create_skill(sources, "nexum-graph-sprint")
            destination = create_skill(destinations, "nexum-graph-sprint")
            (destination / "SKILL.md").write_text("out of sync\n", encoding="utf-8")

            status = sync_nex_skills.evaluate_skill(
                "nexum-graph-sprint",
                sources=sources,
                destinations=destinations,
                check_only=True,
            )
            self.assertEqual(status.status, "out_of_sync")


if __name__ == "__main__":
    unittest.main()

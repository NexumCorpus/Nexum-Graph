from __future__ import annotations

import sys
import tempfile
import unittest
from pathlib import Path

TOOLS_DIR = Path(__file__).resolve().parent
if str(TOOLS_DIR) not in sys.path:
    sys.path.insert(0, str(TOOLS_DIR))

import project_facts


class ProjectFactsTests(unittest.TestCase):
    def test_detect_license_identifies_mit(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            (root / "LICENSE").write_text("MIT License\n", encoding="utf-8")

            self.assertEqual(project_facts.detect_license(root), "MIT")

    def test_count_cli_commands_counts_top_level_variants(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            cli = root / "crates" / "nex-cli" / "src"
            cli.mkdir(parents=True)
            (cli / "cli.rs").write_text(
                "\n".join(
                    [
                        "pub enum Commands {",
                        "    Demo,",
                        "    Start { format: String },",
                        "    Auth {",
                        "        #[command(subcommand)]",
                        "        command: AuthCommands,",
                        "    },",
                        "}",
                    ]
                ),
                encoding="utf-8",
            )

            self.assertEqual(project_facts.count_cli_commands(root), 3)

    def test_render_readme_facts_contains_expected_counts(self) -> None:
        facts = project_facts.ProjectFacts(
            project_name="Nexum Graph",
            license_name="MIT",
            rust_crates=8,
            cli_commands=14,
            rust_tests=223,
            python_tests=30,
            total_tests=253,
        )

        block = project_facts.render_readme_facts(facts)

        self.assertIn("253 source-defined automated tests", block)
        self.assertIn("MIT licensed", block)
        self.assertIn(project_facts.README_FACTS_START, block)
        self.assertIn(project_facts.README_FACTS_END, block)

    def test_sync_and_check_readme_round_trip(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            crates_dir = root / "crates"
            tools_dir = root / "tools"
            cli_src = crates_dir / "nex-cli" / "src"
            cli_src.mkdir(parents=True)
            tools_dir.mkdir()

            (root / "LICENSE").write_text("MIT License\n", encoding="utf-8")
            (cli_src / "cli.rs").write_text(
                "\n".join(
                    [
                        "pub enum Commands {",
                        "    Demo,",
                        "    Check { format: String },",
                        "}",
                    ]
                ),
                encoding="utf-8",
            )
            (crates_dir / "nex-core").mkdir()
            (crates_dir / "nex-core" / "Cargo.toml").write_text("[package]\nname='nex-core'\n", encoding="utf-8")
            (crates_dir / "nex-cli" / "Cargo.toml").write_text("[package]\nname='nex-cli'\n", encoding="utf-8")
            (crates_dir / "nex-cli" / "tests.rs").write_text(
                "#[test]\nfn smoke() {}\n#[tokio::test]\nasync fn async_smoke() {}\n",
                encoding="utf-8",
            )
            (tools_dir / "test_example.py").write_text(
                "def test_alpha():\n    pass\n\ndef test_beta():\n    pass\n",
                encoding="utf-8",
            )
            readme = root / "README.md"
            readme.write_text(
                "\n".join(
                    [
                        "# Nexum Graph",
                        project_facts.README_FACTS_START,
                        "- stale facts",
                        project_facts.README_FACTS_END,
                    ]
                ),
                encoding="utf-8",
            )

            with self.assertRaises(project_facts.ProjectFactsError):
                project_facts.check_readme(root)

            project_facts.sync_readme(root)
            project_facts.check_readme(root)

            synced = readme.read_text(encoding="utf-8")
            self.assertIn("2 Rust crates in one workspace", synced)
            self.assertIn("2 CLI commands", synced)
            self.assertIn("4 source-defined automated tests (2 Rust, 2 Python)", synced)


if __name__ == "__main__":
    unittest.main()

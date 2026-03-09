from __future__ import annotations

import sys
import tarfile
import tempfile
import unittest
import zipfile
from pathlib import Path
from unittest import mock

TOOLS_DIR = Path(__file__).resolve().parent
if str(TOOLS_DIR) not in sys.path:
    sys.path.insert(0, str(TOOLS_DIR))

import release_tools


class ReleaseToolsTests(unittest.TestCase):
    def test_assert_version_parity_matches_repo_manifests(self) -> None:
        self.assertEqual(release_tools.assert_version_parity("v0.1.0"), "0.1.0")

    def test_assert_version_parity_rejects_manifest_drift(self) -> None:
        with mock.patch.object(
            release_tools,
            "load_versions",
            return_value={
                "nex-cli": "0.1.0",
                "nex-lsp": "0.1.1",
                "nexum-graph-vscode": "0.1.0",
            },
        ):
            with self.assertRaisesRegex(release_tools.ReleaseError, "version mismatch"):
                release_tools.assert_version_parity()

    def test_assert_version_parity_rejects_tag_mismatch(self) -> None:
        with self.assertRaisesRegex(release_tools.ReleaseError, "does not match manifest version"):
            release_tools.assert_version_parity("v0.2.0")

    def test_release_asset_names_follow_contract(self) -> None:
        self.assertEqual(
            release_tools.release_asset_names("0.1.0"),
            [
                "nexum-graph-v0.1.0-aarch64-apple-darwin.tar.gz",
                "nexum-graph-v0.1.0-x86_64-apple-darwin.tar.gz",
                "nexum-graph-v0.1.0-x86_64-pc-windows-msvc.zip",
                "nexum-graph-v0.1.0-x86_64-unknown-linux-gnu.tar.gz",
                "nexum-graph-vscode-0.1.0.vsix",
                "SHA256SUMS.txt",
            ],
        )

    def test_package_bundle_creates_linux_archive_with_expected_files(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            source_dir = root / "bin"
            output_dir = root / "dist"
            source_dir.mkdir()
            (source_dir / "nex").write_text("#!/bin/sh\necho nex\n", encoding="utf-8")
            (source_dir / "nex-lsp").write_text("#!/bin/sh\necho nex-lsp\n", encoding="utf-8")
            (source_dir / "nex-lsp-fake-upstream").write_text("ignored\n", encoding="utf-8")
            license_path = root / "LICENSE"
            install_note = root / "INSTALL.txt"
            license_path.write_text("MIT\n", encoding="utf-8")
            install_note.write_text("Run nex demo\n", encoding="utf-8")

            archive_path = release_tools.package_bundle(
                version="0.1.0",
                target="x86_64-unknown-linux-gnu",
                source_dir=source_dir,
                output_dir=output_dir,
                license_path=license_path,
                install_note_path=install_note,
            )

            self.assertEqual(archive_path.name, "nexum-graph-v0.1.0-x86_64-unknown-linux-gnu.tar.gz")
            with tarfile.open(archive_path, mode="r:gz") as archive:
                names = sorted(archive.getnames())
            self.assertEqual(
                names,
                [
                    "nexum-graph-v0.1.0-x86_64-unknown-linux-gnu",
                    "nexum-graph-v0.1.0-x86_64-unknown-linux-gnu/INSTALL.txt",
                    "nexum-graph-v0.1.0-x86_64-unknown-linux-gnu/LICENSE",
                    "nexum-graph-v0.1.0-x86_64-unknown-linux-gnu/nex",
                    "nexum-graph-v0.1.0-x86_64-unknown-linux-gnu/nex-lsp",
                ],
            )

    def test_package_bundle_creates_windows_zip_with_expected_files(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            source_dir = root / "bin"
            output_dir = root / "dist"
            source_dir.mkdir()
            (source_dir / "nex.exe").write_bytes(b"nex")
            (source_dir / "nex-lsp.exe").write_bytes(b"lsp")
            license_path = root / "LICENSE"
            install_note = root / "INSTALL.txt"
            license_path.write_text("MIT\n", encoding="utf-8")
            install_note.write_text("Run nex demo\n", encoding="utf-8")

            archive_path = release_tools.package_bundle(
                version="0.1.0",
                target="x86_64-pc-windows-msvc",
                source_dir=source_dir,
                output_dir=output_dir,
                license_path=license_path,
                install_note_path=install_note,
            )

            self.assertEqual(archive_path.name, "nexum-graph-v0.1.0-x86_64-pc-windows-msvc.zip")
            with zipfile.ZipFile(archive_path, mode="r") as archive:
                names = sorted(archive.namelist())
            self.assertEqual(
                names,
                [
                    "nexum-graph-v0.1.0-x86_64-pc-windows-msvc/INSTALL.txt",
                    "nexum-graph-v0.1.0-x86_64-pc-windows-msvc/LICENSE",
                    "nexum-graph-v0.1.0-x86_64-pc-windows-msvc/nex-lsp.exe",
                    "nexum-graph-v0.1.0-x86_64-pc-windows-msvc/nex.exe",
                ],
            )

    def test_write_checksums_creates_sha256_manifest(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            first = root / "alpha.txt"
            second = root / "beta.txt"
            output = root / "SHA256SUMS.txt"
            first.write_text("alpha\n", encoding="utf-8")
            second.write_text("beta\n", encoding="utf-8")

            release_tools.write_checksums([second, first], output)
            lines = output.read_text(encoding="utf-8").strip().splitlines()

            self.assertEqual(len(lines), 2)
            self.assertTrue(lines[0].endswith("  alpha.txt"))
            self.assertTrue(lines[1].endswith("  beta.txt"))


if __name__ == "__main__":
    unittest.main()

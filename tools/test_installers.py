from __future__ import annotations

import http.server
import json
import os
import shutil
import socketserver
import subprocess
import sys
import tempfile
import threading
import unittest
from pathlib import Path

TOOLS_DIR = Path(__file__).resolve().parent
REPO_ROOT = TOOLS_DIR.parent
if str(TOOLS_DIR) not in sys.path:
    sys.path.insert(0, str(TOOLS_DIR))

import release_tools


class InstallerServer:
    def __init__(self, root: Path) -> None:
        handler = self._handler_factory(root)
        self._server = socketserver.TCPServer(("127.0.0.1", 0), handler)
        self.port = self._server.server_address[1]
        self._thread = threading.Thread(target=self._server.serve_forever, daemon=True)

    def _handler_factory(self, root: Path):
        class Handler(http.server.SimpleHTTPRequestHandler):
            def __init__(self, *args, **kwargs):
                super().__init__(*args, directory=str(root), **kwargs)

            def log_message(self, format: str, *args) -> None:
                return

        return Handler

    def __enter__(self) -> "InstallerServer":
        self._thread.start()
        return self

    def __exit__(self, exc_type, exc, tb) -> None:
        self._server.shutdown()
        self._server.server_close()
        self._thread.join(timeout=5)


class InstallerTests(unittest.TestCase):
    def build_release_fixture(self, root: Path, target: str, binaries: dict[str, bytes]) -> str:
        version = "0.1.0"
        source_dir = root / "source"
        source_dir.mkdir()
        for name, payload in binaries.items():
            mode = "wb"
            with (source_dir / name).open(mode) as handle:
                handle.write(payload)

        bundle_dir = root / "download" / f"v{version}"
        bundle_dir.mkdir(parents=True)
        archive_path = release_tools.package_bundle(
            version=version,
            target=target,
            source_dir=source_dir,
            output_dir=bundle_dir,
            license_path=REPO_ROOT / "LICENSE",
            install_note_path=REPO_ROOT / "packaging" / "INSTALL.txt",
        )
        release_tools.write_checksums([archive_path], bundle_dir / "SHA256SUMS.txt")

        api_dir = root / "api" / "releases"
        api_dir.mkdir(parents=True)
        (api_dir / "latest.json").write_text(json.dumps({"tag_name": f"v{version}"}), encoding="utf-8")
        return version

    @unittest.skipUnless(sys.platform.startswith("win"), "PowerShell installer smoke test is Windows-only.")
    def test_powershell_installer_downloads_and_installs_latest_release(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            install_dir = root / "bin"
            version = self.build_release_fixture(
                root,
                "x86_64-pc-windows-msvc",
                {
                    "nex.exe": b"nex",
                    "nex-lsp.exe": b"nex-lsp",
                },
            )

            with InstallerServer(root) as server:
                env = os.environ.copy()
                env["NEXUM_GRAPH_RELEASE_BASE_URL"] = f"http://127.0.0.1:{server.port}"
                env["NEXUM_GRAPH_API_URL"] = f"http://127.0.0.1:{server.port}/api/releases/latest.json"
                completed = subprocess.run(
                    [
                        "powershell",
                        "-ExecutionPolicy",
                        "Bypass",
                        "-File",
                        str(REPO_ROOT / "install.ps1"),
                        "-InstallDir",
                        str(install_dir),
                        "-Force",
                    ],
                    cwd=REPO_ROOT,
                    capture_output=True,
                    text=True,
                    env=env,
                    check=False,
                )

            self.assertEqual(completed.returncode, 0, msg=completed.stderr or completed.stdout)
            self.assertTrue((install_dir / "nex.exe").exists())
            self.assertTrue((install_dir / "nex-lsp.exe").exists())

    @unittest.skipIf(sys.platform.startswith("win"), "POSIX installer smoke test runs on Unix-like platforms.")
    def test_shell_installer_downloads_and_installs_latest_release(self) -> None:
        bash = shutil.which("bash")
        if bash is None:
            self.skipTest("bash not available")

        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            install_dir = root / "bin"
            self.build_release_fixture(
                root,
                "x86_64-unknown-linux-gnu",
                {
                    "nex": b"#!/bin/sh\necho nex\n",
                    "nex-lsp": b"#!/bin/sh\necho nex-lsp\n",
                },
            )

            with InstallerServer(root) as server:
                env = os.environ.copy()
                env["NEXUM_GRAPH_RELEASE_BASE_URL"] = f"http://127.0.0.1:{server.port}"
                env["NEXUM_GRAPH_API_URL"] = f"http://127.0.0.1:{server.port}/api/releases/latest.json"
                completed = subprocess.run(
                    [bash, str(REPO_ROOT / "install.sh"), "--install-dir", str(install_dir), "--force"],
                    cwd=REPO_ROOT,
                    capture_output=True,
                    text=True,
                    env=env,
                    check=False,
                )

            self.assertEqual(completed.returncode, 0, msg=completed.stderr or completed.stdout)
            self.assertTrue((install_dir / "nex").exists())
            self.assertTrue((install_dir / "nex-lsp").exists())


if __name__ == "__main__":
    unittest.main()

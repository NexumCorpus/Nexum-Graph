#!/usr/bin/env python3
"""Release helpers for Nexum Graph GitHub Releases."""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import shutil
import sys
import tarfile
import tempfile
import tomllib
import zipfile
from dataclasses import dataclass
from pathlib import Path

SEMVER_RE = re.compile(r"^v?(?P<version>\d+\.\d+\.\d+)$")


@dataclass(frozen=True)
class ReleaseTarget:
    target: str
    archive_format: str
    executable_suffix: str


SUPPORTED_TARGETS = {
    "x86_64-unknown-linux-gnu": ReleaseTarget(
        target="x86_64-unknown-linux-gnu",
        archive_format="tar.gz",
        executable_suffix="",
    ),
    "x86_64-pc-windows-msvc": ReleaseTarget(
        target="x86_64-pc-windows-msvc",
        archive_format="zip",
        executable_suffix=".exe",
    ),
    "x86_64-apple-darwin": ReleaseTarget(
        target="x86_64-apple-darwin",
        archive_format="tar.gz",
        executable_suffix="",
    ),
    "aarch64-apple-darwin": ReleaseTarget(
        target="aarch64-apple-darwin",
        archive_format="tar.gz",
        executable_suffix="",
    ),
}


class ReleaseError(RuntimeError):
    """Raised when release packaging or validation fails."""


def repo_root() -> Path:
    return Path(__file__).resolve().parent.parent


def cli_manifest_path(root: Path | None = None) -> Path:
    return (root or repo_root()) / "crates" / "nex-cli" / "Cargo.toml"


def lsp_manifest_path(root: Path | None = None) -> Path:
    return (root or repo_root()) / "crates" / "nex-lsp" / "Cargo.toml"


def vscode_manifest_path(root: Path | None = None) -> Path:
    return (root or repo_root()) / "extensions" / "vscode" / "package.json"


def default_license_path(root: Path | None = None) -> Path:
    return (root or repo_root()) / "LICENSE"


def default_install_note_path(root: Path | None = None) -> Path:
    return (root or repo_root()) / "packaging" / "INSTALL.txt"


def normalize_version(value: str) -> str:
    match = SEMVER_RE.fullmatch(value.strip())
    if not match:
        raise ReleaseError(f"expected semantic version or tag like 0.1.0 / v0.1.0, got: {value}")
    return match.group("version")


def normalize_tag(value: str) -> str:
    return f"v{normalize_version(value)}"


def read_cargo_package_version(path: Path) -> str:
    with path.open("rb") as handle:
        manifest = tomllib.load(handle)
    try:
        return str(manifest["package"]["version"])
    except KeyError as err:
        raise ReleaseError(f"missing package.version in {path}") from err


def read_vscode_extension_version(path: Path) -> str:
    try:
        payload = json.loads(path.read_text(encoding="utf-8"))
    except json.JSONDecodeError as err:
        raise ReleaseError(f"invalid JSON in {path}: {err}") from err
    version = payload.get("version")
    if not isinstance(version, str) or not version.strip():
        raise ReleaseError(f"missing version in {path}")
    return normalize_version(version)


def load_versions(root: Path | None = None) -> dict[str, str]:
    root = root or repo_root()
    return {
        "nex-cli": normalize_version(read_cargo_package_version(cli_manifest_path(root))),
        "nex-lsp": normalize_version(read_cargo_package_version(lsp_manifest_path(root))),
        "nexum-graph-vscode": read_vscode_extension_version(vscode_manifest_path(root)),
    }


def assert_version_parity(expected_tag: str | None = None, root: Path | None = None) -> str:
    versions = load_versions(root)
    unique_versions = sorted(set(versions.values()))
    if len(unique_versions) != 1:
        detail = ", ".join(f"{name}={version}" for name, version in sorted(versions.items()))
        raise ReleaseError(f"version mismatch across release artifacts: {detail}")

    version = unique_versions[0]
    if expected_tag is not None and normalize_version(expected_tag) != version:
        raise ReleaseError(
            f"tag {normalize_tag(expected_tag)} does not match manifest version {version}"
        )
    return version


def target_info(target: str) -> ReleaseTarget:
    try:
        return SUPPORTED_TARGETS[target]
    except KeyError as err:
        supported = ", ".join(sorted(SUPPORTED_TARGETS))
        raise ReleaseError(f"unsupported target {target!r}; expected one of: {supported}") from err


def release_bundle_name(version: str, target: str) -> str:
    info = target_info(target)
    return f"nexum-graph-v{normalize_version(version)}-{info.target}.{info.archive_format}"


def vscode_asset_name(version: str) -> str:
    return f"nexum-graph-vscode-{normalize_version(version)}.vsix"


def checksum_asset_name() -> str:
    return "SHA256SUMS.txt"


def release_asset_names(version: str) -> list[str]:
    version = normalize_version(version)
    names = [release_bundle_name(version, target) for target in sorted(SUPPORTED_TARGETS)]
    names.append(vscode_asset_name(version))
    names.append(checksum_asset_name())
    return names


def bundled_binary_names(target: str) -> list[str]:
    suffix = target_info(target).executable_suffix
    return [f"nex{suffix}", f"nex-lsp{suffix}"]


def package_bundle(
    version: str,
    target: str,
    source_dir: Path,
    output_dir: Path,
    license_path: Path | None = None,
    install_note_path: Path | None = None,
) -> Path:
    version = normalize_version(version)
    info = target_info(target)
    license_path = license_path or default_license_path()
    install_note_path = install_note_path or default_install_note_path()

    if not license_path.exists():
        raise ReleaseError(f"missing license file: {license_path}")
    if not install_note_path.exists():
        raise ReleaseError(f"missing install note: {install_note_path}")

    archive_name = release_bundle_name(version, target)
    output_dir.mkdir(parents=True, exist_ok=True)
    archive_path = output_dir / archive_name
    bundle_root_name = archive_name.removesuffix(f".{info.archive_format}")

    with tempfile.TemporaryDirectory() as temp_dir:
        temp_root = Path(temp_dir)
        bundle_root = temp_root / bundle_root_name
        bundle_root.mkdir()

        for binary in bundled_binary_names(target):
            source_path = source_dir / binary
            if not source_path.exists():
                raise ReleaseError(f"missing binary for bundle: {source_path}")
            shutil.copy2(source_path, bundle_root / binary)

        shutil.copy2(license_path, bundle_root / "LICENSE")
        shutil.copy2(install_note_path, bundle_root / "INSTALL.txt")

        if info.archive_format == "zip":
            with zipfile.ZipFile(
                archive_path,
                mode="w",
                compression=zipfile.ZIP_DEFLATED,
            ) as archive:
                for path in sorted(bundle_root.rglob("*")):
                    if path.is_file():
                        archive.write(path, path.relative_to(temp_root))
        else:
            with tarfile.open(archive_path, mode="w:gz") as archive:
                archive.add(bundle_root, arcname=bundle_root_name)

    return archive_path


def write_checksums(paths: list[Path], output_path: Path) -> Path:
    lines = []
    for path in sorted(paths, key=lambda item: item.name):
        digest = hashlib.sha256(path.read_bytes()).hexdigest()
        lines.append(f"{digest}  {path.name}")
    output_path.parent.mkdir(parents=True, exist_ok=True)
    output_path.write_text("\n".join(lines) + "\n", encoding="utf-8")
    return output_path


def manifest_payload(version: str) -> dict[str, object]:
    version = normalize_version(version)
    return {
        "version": version,
        "tag": normalize_tag(version),
        "targets": sorted(SUPPORTED_TARGETS),
        "bundles": [release_bundle_name(version, target) for target in sorted(SUPPORTED_TARGETS)],
        "vscode_asset": vscode_asset_name(version),
        "checksum_asset": checksum_asset_name(),
    }


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Release helpers for Nexum Graph.")
    subparsers = parser.add_subparsers(dest="command", required=True)

    parity = subparsers.add_parser("assert-version-parity", help="Validate release version parity.")
    parity.add_argument("--tag", help="Expected git tag in vX.Y.Z form.")
    parity.add_argument("--json", action="store_true", help="Print JSON payload.")

    manifest = subparsers.add_parser("print-manifest", help="Print release asset manifest.")
    manifest.add_argument("--tag", help="Expected git tag in vX.Y.Z form.")
    manifest.add_argument("--json", action="store_true", help="Print JSON payload.")

    package = subparsers.add_parser("package-bundle", help="Create a bundled release archive.")
    package.add_argument("--version", required=True, help="Release version in X.Y.Z form.")
    package.add_argument("--target", required=True, help="Rust target triple.")
    package.add_argument("--source-dir", required=True, type=Path, help="Directory holding built binaries.")
    package.add_argument("--output-dir", required=True, type=Path, help="Directory for release assets.")
    package.add_argument("--license", type=Path, default=default_license_path(), help="License file to bundle.")
    package.add_argument(
        "--install-note",
        type=Path,
        default=default_install_note_path(),
        help="Short install note to bundle as INSTALL.txt.",
    )

    checksums = subparsers.add_parser("write-checksums", help="Write SHA256SUMS.txt for release assets.")
    checksums.add_argument("--output", required=True, type=Path, help="Output checksum manifest path.")
    checksums.add_argument("paths", nargs="+", type=Path, help="Files to include in the manifest.")

    return parser.parse_args()


def main() -> int:
    args = parse_args()
    try:
        if args.command == "assert-version-parity":
            version = assert_version_parity(args.tag)
            if args.json:
                print(json.dumps({"version": version, "tag": normalize_tag(version)}, indent=2))
            else:
                print(version)
            return 0

        if args.command == "print-manifest":
            version = assert_version_parity(args.tag)
            payload = manifest_payload(version)
            if args.json:
                print(json.dumps(payload, indent=2))
            else:
                print(f"version={payload['version']}")
                print(f"tag={payload['tag']}")
                for bundle in payload["bundles"]:
                    print(bundle)
                print(payload["vscode_asset"])
                print(payload["checksum_asset"])
            return 0

        if args.command == "package-bundle":
            archive_path = package_bundle(
                version=args.version,
                target=args.target,
                source_dir=args.source_dir,
                output_dir=args.output_dir,
                license_path=args.license,
                install_note_path=args.install_note,
            )
            print(str(archive_path))
            return 0

        if args.command == "write-checksums":
            output_path = write_checksums(args.paths, args.output)
            print(str(output_path))
            return 0
    except ReleaseError as err:
        print(f"error: {err}", file=sys.stderr)
        return 1

    raise AssertionError(f"unhandled command: {args.command}")


if __name__ == "__main__":
    raise SystemExit(main())

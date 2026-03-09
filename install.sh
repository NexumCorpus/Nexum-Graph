#!/usr/bin/env sh
set -eu

DEFAULT_RELEASE_BASE_URL="https://github.com/NexumCorpus/Nexum-Graph/releases"
DEFAULT_API_URL="https://api.github.com/repos/NexumCorpus/Nexum-Graph/releases/latest"

VERSION=""
INSTALL_DIR="${HOME}/.local/bin"
FORCE=0

usage() {
    cat <<'EOF'
Install Nexum Graph from GitHub Releases.

Usage:
  install.sh [--version X.Y.Z] [--install-dir PATH] [--force]

Options:
  --version X.Y.Z      Install a specific release instead of the latest release.
  --install-dir PATH   Install nex and nex-lsp into PATH.
  --force              Overwrite existing binaries in the install directory.
  -h, --help           Show this help.
EOF
}

fail() {
    printf 'error: %s\n' "$*" >&2
    exit 1
}

need_command() {
    command -v "$1" >/dev/null 2>&1 || fail "missing required command: $1"
}

download_file() {
    url="$1"
    destination="$2"
    if command -v curl >/dev/null 2>&1; then
        curl -fsSL -H 'User-Agent: nexum-graph-installer' "$url" -o "$destination"
        return
    fi
    if command -v wget >/dev/null 2>&1; then
        wget -qO "$destination" "$url"
        return
    fi
    fail "missing downloader: install curl or wget"
}

normalize_version() {
    value="$1"
    normalized=$(printf '%s' "$value" | sed -E 's/^v([0-9]+\.[0-9]+\.[0-9]+)$/\1/;t;s/^([0-9]+\.[0-9]+\.[0-9]+)$/\1/;t;d')
    [ -n "$normalized" ] || fail "expected version like 0.1.0 or v0.1.0, got: $value"
    printf '%s\n' "$normalized"
}

fetch_latest_version() {
    release_base_url="${NEXUM_GRAPH_RELEASE_BASE_URL:-$DEFAULT_RELEASE_BASE_URL}"
    api_url="${NEXUM_GRAPH_API_URL:-$DEFAULT_API_URL}"

    if [ "$release_base_url" != "$DEFAULT_RELEASE_BASE_URL" ] && [ -z "${NEXUM_GRAPH_API_URL:-}" ]; then
        fail "version is required when NEXUM_GRAPH_RELEASE_BASE_URL is overridden"
    fi

    response_file="$1"
    download_file "$api_url" "$response_file"
    tag=$(sed -nE 's/.*"tag_name"[[:space:]]*:[[:space:]]*"v([0-9]+\.[0-9]+\.[0-9]+)".*/\1/p' "$response_file" | head -n 1)
    [ -n "$tag" ] || fail "could not determine latest release version from $api_url"
    printf '%s\n' "$tag"
}

resolve_target() {
    os_name=$(uname -s)
    arch_name=$(uname -m)

    case "$os_name" in
        Linux) os_part="unknown-linux-gnu" ;;
        Darwin) os_part="apple-darwin" ;;
        *) fail "unsupported operating system: $os_name" ;;
    esac

    case "$arch_name" in
        x86_64|amd64) arch_part="x86_64" ;;
        arm64|aarch64) arch_part="aarch64" ;;
        *) fail "unsupported architecture: $arch_name" ;;
    esac

    target="${arch_part}-${os_part}"
    case "$target" in
        x86_64-unknown-linux-gnu|x86_64-apple-darwin|aarch64-apple-darwin)
            printf '%s\n' "$target"
            ;;
        *)
            fail "unsupported platform: $target"
            ;;
    esac
}

archive_name_for_target() {
    version="$1"
    target="$2"
    printf 'nexum-graph-v%s-%s.tar.gz\n' "$version" "$target"
}

expected_checksum() {
    checksum_file="$1"
    asset_name="$2"
    checksum=$(awk -v name="$asset_name" '$2 == name { print $1 }' "$checksum_file")
    [ -n "$checksum" ] || fail "missing checksum entry for $asset_name"
    printf '%s\n' "$checksum"
}

actual_checksum() {
    file_path="$1"
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$file_path" | awk '{ print $1 }'
        return
    fi
    if command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "$file_path" | awk '{ print $1 }'
        return
    fi
    fail "missing checksum tool: install sha256sum or shasum"
}

copy_binary() {
    source_path="$1"
    destination_path="$2"
    if [ -e "$destination_path" ] && [ "$FORCE" -ne 1 ]; then
        fail "refusing to overwrite $destination_path without --force"
    fi
    mkdir -p "$(dirname "$destination_path")"
    cp "$source_path" "$destination_path"
    chmod 755 "$destination_path" 2>/dev/null || true
}

while [ $# -gt 0 ]; do
    case "$1" in
        --version)
            [ $# -ge 2 ] || fail "--version requires a value"
            VERSION=$(normalize_version "$2")
            shift 2
            ;;
        --install-dir)
            [ $# -ge 2 ] || fail "--install-dir requires a value"
            INSTALL_DIR="$2"
            shift 2
            ;;
        --force)
            FORCE=1
            shift
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            fail "unknown argument: $1"
            ;;
    esac
done

need_command tar

tmp_dir=$(mktemp -d 2>/dev/null || mktemp -d -t nexum-graph-install)
cleanup() {
    rm -rf "$tmp_dir"
}
trap cleanup EXIT INT TERM

release_base_url=${NEXUM_GRAPH_RELEASE_BASE_URL:-$DEFAULT_RELEASE_BASE_URL}
release_base_url=$(printf '%s' "$release_base_url" | sed 's:/*$::')

if [ -z "$VERSION" ]; then
    VERSION=$(fetch_latest_version "$tmp_dir/latest-release.json")
fi

VERSION=$(normalize_version "$VERSION")
TAG="v$VERSION"
TARGET=$(resolve_target)
ARCHIVE_NAME=$(archive_name_for_target "$VERSION" "$TARGET")

CHECKSUM_URL="$release_base_url/download/$TAG/SHA256SUMS.txt"
ARCHIVE_URL="$release_base_url/download/$TAG/$ARCHIVE_NAME"

CHECKSUM_FILE="$tmp_dir/SHA256SUMS.txt"
ARCHIVE_FILE="$tmp_dir/$ARCHIVE_NAME"
EXTRACT_DIR="$tmp_dir/extract"

mkdir -p "$EXTRACT_DIR"
download_file "$CHECKSUM_URL" "$CHECKSUM_FILE"
download_file "$ARCHIVE_URL" "$ARCHIVE_FILE"

EXPECTED_CHECKSUM=$(expected_checksum "$CHECKSUM_FILE" "$ARCHIVE_NAME")
ACTUAL_CHECKSUM=$(actual_checksum "$ARCHIVE_FILE")
[ "$EXPECTED_CHECKSUM" = "$ACTUAL_CHECKSUM" ] || fail "checksum mismatch for $ARCHIVE_NAME"

tar -xzf "$ARCHIVE_FILE" -C "$EXTRACT_DIR"
BUNDLE_ROOT=$(find "$EXTRACT_DIR" -mindepth 1 -maxdepth 1 -type d | head -n 1)
[ -n "$BUNDLE_ROOT" ] || fail "release archive did not contain a bundle directory"

copy_binary "$BUNDLE_ROOT/nex" "$INSTALL_DIR/nex"
copy_binary "$BUNDLE_ROOT/nex-lsp" "$INSTALL_DIR/nex-lsp"

printf 'Installed nex and nex-lsp to %s\n' "$INSTALL_DIR"
if command -v "$INSTALL_DIR/nex" >/dev/null 2>&1; then
    "$INSTALL_DIR/nex" --version || true
fi

case ":${PATH:-}:" in
    *":$INSTALL_DIR:"*) ;;
    *)
        printf 'Add %s to PATH, for example:\n' "$INSTALL_DIR"
        printf '  export PATH="%s:$PATH"\n' "$INSTALL_DIR"
        ;;
esac

#!/usr/bin/env bash
# Download a prebuilt `thf` backend binary for this platform and install it to
# bin/thf next to the plugin.
#
# This runs on its own (prefetch right after `git clone`, or from a TPM
# post-install hook) and is also used by history_finder.sh as a last-resort
# fallback when no toolchain is available.
#
# Env:
#   THF_REPO      override "owner/repo"            (default: hmgle/tmux-history-finder)
#   THF_VERSION   override the version to download (default: read from Cargo.toml)
#   THF_BASE_URL  override the release asset base URL (for mirrors/air-gapped use;
#                 default: https://github.com/<repo>/releases/download/v<version>)
# Flags:
#   --force      re-download even if bin/thf already exists

set -o pipefail

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REPO="${THF_REPO:-hmgle/tmux-history-finder}"
DEST="$DIR/bin/thf"

force=0
[ "${1:-}" = "--force" ] && force=1

if [ "$force" -eq 0 ] && [ -x "$DEST" ]; then
    echo "install-binary: thf already present at $DEST (use --force to reinstall)." >&2
    exit 0
fi

# Version to download. The checked-out source corresponds to a specific release,
# so pin to Cargo.toml's version unless the caller overrides it.
version="${THF_VERSION:-}"
if [ -z "$version" ] && [ -f "$DIR/Cargo.toml" ]; then
    version="$(awk -F'"' '/^version[[:space:]]*=/{print $2; exit}' "$DIR/Cargo.toml")"
fi
if [ -z "$version" ]; then
    echo "install-binary: could not determine which version to download." >&2
    exit 1
fi
tag="v$version"

# Map this platform to a release target triple (must match release.yml assets).
os="$(uname -s)"
arch="$(uname -m)"
case "$os" in
    Linux)  os_part="unknown-linux-musl" ;;
    Darwin) os_part="apple-darwin" ;;
    *) echo "install-binary: unsupported OS '$os'; build from source instead." >&2; exit 1 ;;
esac
case "$arch" in
    x86_64|amd64)  arch_part="x86_64" ;;
    aarch64|arm64) arch_part="aarch64" ;;
    *) echo "install-binary: unsupported architecture '$arch'; build from source instead." >&2; exit 1 ;;
esac
target="${arch_part}-${os_part}"
asset="thf-${target}.tar.gz"
base="${THF_BASE_URL:-https://github.com/${REPO}/releases/download/${tag}}"

fetch() { # url dest
    if command -v curl >/dev/null 2>&1; then
        curl -fsSL "$1" -o "$2"
    elif command -v wget >/dev/null 2>&1; then
        wget -q -O "$2" "$1"
    else
        echo "install-binary: need curl or wget to download a prebuilt binary." >&2
        return 127
    fi
}

sha256_of() { # file -> hex on stdout
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | awk '{print $1}'
    elif command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "$1" | awk '{print $1}'
    else
        return 1
    fi
}

tmp="$(mktemp -d "${TMPDIR:-/tmp}/thf-install.XXXXXX")" || exit 1
trap 'rm -rf "$tmp"' EXIT

echo "install-binary: downloading $asset ($tag)..." >&2
if ! fetch "${base}/${asset}" "$tmp/$asset"; then
    echo "install-binary: download failed: ${base}/${asset}" >&2
    echo "  No prebuilt binary for $target at $tag? Build from source: cargo build --release" >&2
    exit 1
fi

# Verify the checksum when one is published alongside the asset (best effort:
# skip silently if no .sha256 sidecar or no local hashing tool).
if fetch "${base}/${asset}.sha256" "$tmp/$asset.sha256" 2>/dev/null; then
    expected="$(awk '{print $1}' "$tmp/$asset.sha256")"
    if actual="$(sha256_of "$tmp/$asset")" && [ -n "$expected" ] && [ "$expected" != "$actual" ]; then
        echo "install-binary: checksum mismatch (expected $expected, got $actual)." >&2
        exit 1
    fi
fi

if ! tar -xzf "$tmp/$asset" -C "$tmp"; then
    echo "install-binary: failed to extract $asset." >&2
    exit 1
fi

# The tarball holds thf-<target>/thf; fall back to a search just in case.
src="$tmp/thf-${target}/thf"
[ -f "$src" ] || src="$(find "$tmp" -type f -name thf 2>/dev/null | head -n1)"
if [ -z "$src" ] || [ ! -f "$src" ]; then
    echo "install-binary: 'thf' not found inside the archive." >&2
    exit 1
fi

mkdir -p "$DIR/bin"
cp "$src" "$DEST"
chmod +x "$DEST"
# Best effort: clear the macOS quarantine flag so the binary runs without a prompt.
[ "$os" = "Darwin" ] && xattr -d com.apple.quarantine "$DEST" 2>/dev/null
true

echo "install-binary: installed thf $version to $DEST" >&2

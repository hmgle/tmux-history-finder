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

set -euo pipefail

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REPO="${THF_REPO:-hmgle/tmux-history-finder}"
DEST="$DIR/bin/thf"

force=0
case "${1:-}" in
    "") ;;
    --force) force=1 ;;
    *) echo "install-binary: unknown argument '$1'." >&2; exit 2 ;;
esac

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

if [ -z "${THF_VERSION:-}" ] && command -v git >/dev/null 2>&1 &&
    git -C "$DIR" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
    checkout_tag="$(git -C "$DIR" describe --exact-match --tags HEAD 2>/dev/null || true)"
    if [ "$checkout_tag" != "$tag" ]; then
        echo "install-binary: refusing $tag for an untagged source checkout." >&2
        echo "  Build the current source with cargo, or install a tagged release." >&2
        exit 1
    fi
fi

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
mkdir -p "$DIR/bin"
lock="$DIR/bin/.install.lock"
lock_held=0
attempts=0
max_lock_attempts="${THF_INSTALL_LOCK_ATTEMPTS:-300}"
case "$max_lock_attempts" in
    ''|*[!0-9]*) echo "install-binary: THF_INSTALL_LOCK_ATTEMPTS must be numeric." >&2; exit 2 ;;
esac
while [ "$attempts" -lt "$max_lock_attempts" ]; do
    if mkdir "$lock" 2>/dev/null; then
        lock_held=1
        break
    fi
    sleep 0.1
    attempts=$((attempts + 1))
done
if [ "$lock_held" -ne 1 ]; then
    echo "install-binary: timed out waiting for another installation." >&2
    exit 1
fi
installed_tmp=""
cleanup() {
    [ -z "$installed_tmp" ] || rm -f "$installed_tmp"
    rm -rf "$tmp"
    rmdir "$lock" 2>/dev/null || true
}
trap cleanup EXIT
trap 'exit 130' HUP INT TERM

installed_version=""
if [ -x "$DEST" ]; then
    installed_version="$("$DEST" --version 2>/dev/null | awk '{print $NF; exit}' || true)"
fi
if [ "$force" -eq 0 ] && [ "$installed_version" = "$version" ]; then
    echo "install-binary: thf $version already present at $DEST." >&2
    exit 0
fi

echo "install-binary: downloading $asset ($tag)..." >&2
if ! fetch "${base}/${asset}" "$tmp/$asset"; then
    echo "install-binary: download failed: ${base}/${asset}" >&2
    echo "  No prebuilt binary for $target at $tag? Build from source: cargo build --release" >&2
    exit 1
fi

# Checksums are mandatory because this path installs executable code.
if ! fetch "${base}/${asset}.sha256" "$tmp/$asset.sha256" 2>/dev/null; then
    echo "install-binary: checksum download failed." >&2
    exit 1
fi
expected="$(awk 'NR == 1 {print $1}' "$tmp/$asset.sha256")"
if ! printf '%s\n' "$expected" | grep -Eq '^[0-9A-Fa-f]{64}$'; then
    echo "install-binary: malformed checksum sidecar." >&2
    exit 1
fi
if ! actual="$(sha256_of "$tmp/$asset")"; then
    echo "install-binary: need sha256sum or shasum to verify the download." >&2
    exit 1
fi
if [ "$expected" != "$actual" ]; then
    echo "install-binary: checksum mismatch (expected $expected, got $actual)." >&2
    exit 1
fi

if ! tar -tzf "$tmp/$asset" > "$tmp/archive.list"; then
    echo "install-binary: failed to inspect $asset." >&2
    exit 1
fi
if awk '$0 ~ /^\// || $0 ~ /(^|\/)\.\.(\/|$)/ { bad = 1 } END { exit bad }' \
    "$tmp/archive.list"; then
    :
else
    echo "install-binary: archive contains an unsafe path." >&2
    exit 1
fi
if ! tar -xzf "$tmp/$asset" -C "$tmp"; then
    echo "install-binary: failed to extract $asset." >&2
    exit 1
fi

# The tarball format is fixed by release.yml.
src="$tmp/thf-${target}/thf"
if [ ! -f "$src" ]; then
    echo "install-binary: 'thf' not found inside the archive." >&2
    exit 1
fi

installed_tmp="$(mktemp "$DIR/bin/.thf.XXXXXX")"
install -m 755 "$src" "$installed_tmp"
mv -f "$installed_tmp" "$DEST"
installed_tmp=""
# Best effort: clear the macOS quarantine flag so the binary runs without a prompt.
[ "$os" = "Darwin" ] && xattr -d com.apple.quarantine "$DEST" 2>/dev/null
true

echo "install-binary: installed thf $version to $DEST" >&2

#!/usr/bin/env bash

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TMP="$(mktemp -d "${TMPDIR:-/tmp}/thf-installer-test.XXXXXX")"
trap 'rm -rf "$TMP"' EXIT

case "$(uname -s)" in
    Linux) os_part="unknown-linux-musl" ;;
    Darwin) os_part="apple-darwin" ;;
    *) echo "installer test: unsupported OS" >&2; exit 77 ;;
esac
case "$(uname -m)" in
    x86_64|amd64) arch_part="x86_64" ;;
    aarch64|arm64) arch_part="aarch64" ;;
    *) echo "installer test: unsupported architecture" >&2; exit 77 ;;
esac
target="${arch_part}-${os_part}"
asset="thf-${target}.tar.gz"

repo="$TMP/repo"
assets="$TMP/assets"
dist="$TMP/thf-${target}"
mkdir -p "$repo/scripts" "$assets" "$dist"
cp "$ROOT/Cargo.toml" "$repo/Cargo.toml"
cp "$ROOT/scripts/install-binary.sh" "$repo/scripts/install-binary.sh"
printf '%s\n' '#!/bin/sh' 'echo "tmux-history-finder 0.5.0"' > "$dist/thf"
chmod +x "$dist/thf"
tar -czf "$assets/$asset" -C "$TMP" "thf-${target}"
if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$assets/$asset" > "$assets/$asset.sha256"
else
    shasum -a 256 "$assets/$asset" > "$assets/$asset.sha256"
fi

install_env=(
    THF_VERSION=0.5.0
    THF_BASE_URL="file://$assets"
    THF_INSTALL_LOCK_ATTEMPTS=10
)

env "${install_env[@]}" bash "$repo/scripts/install-binary.sh" --force >/dev/null
[ "$("$repo/bin/thf" --version)" = "tmux-history-finder 0.5.0" ]

rm -f "$repo/bin/thf"
env "${install_env[@]}" bash "$repo/scripts/install-binary.sh" --force >/dev/null &
first=$!
env "${install_env[@]}" bash "$repo/scripts/install-binary.sh" --force >/dev/null &
second=$!
wait "$first"
wait "$second"
[ "$("$repo/bin/thf" --version)" = "tmux-history-finder 0.5.0" ]

rm -f "$repo/bin/thf" "$assets/$asset.sha256"
if env "${install_env[@]}" bash "$repo/scripts/install-binary.sh" --force >/dev/null 2>&1; then
    echo "installer test: missing checksum unexpectedly succeeded" >&2
    exit 1
fi

mkdir -p "$repo/bin"
chmod 500 "$repo/bin"
if env "${install_env[@]}" THF_INSTALL_LOCK_ATTEMPTS=1 \
    bash "$repo/scripts/install-binary.sh" --force >/dev/null 2>&1; then
    chmod 700 "$repo/bin"
    echo "installer test: unwritable destination unexpectedly succeeded" >&2
    exit 1
fi
chmod 700 "$repo/bin"

echo "installer tests passed"

#!/usr/bin/env bash
# Compatibility entry point for tmux-history-finder.
#
# Prefer a built thf binary when available. In a source checkout, fall back to
# cargo so TPM/manual installs keep working during development.

set -o pipefail

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

find_thf() {
    local candidate
    for candidate in \
        "$DIR/bin/thf" \
        "$DIR/target/release/thf" \
        "$DIR/target/debug/thf"; do
        if [ -x "$candidate" ]; then
            printf '%s\n' "$candidate"
            return 0
        fi
    done
    return 1
}

if thf_bin=$(find_thf); then
    exec "$thf_bin" "$@"
fi

if command -v cargo >/dev/null 2>&1; then
    exec cargo run --quiet --manifest-path "$DIR/Cargo.toml" -- "$@"
fi

echo "history-finder: no thf binary found and cargo is not available." >&2
echo "Build it with: cargo build --release" >&2
exit 1

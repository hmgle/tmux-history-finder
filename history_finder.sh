#!/usr/bin/env bash
# Compatibility entry point for tmux-history-finder.
#
# Resolves a `thf` backend binary and execs it. Resolution order:
#   1. $THF_BIN, when it points at an executable (explicit override).
#   2. A locally built binary under target/ (cargo build output).
#   3. A previously installed binary under bin/ (prebuilt download).
#   4. `cargo run`, when a Rust toolchain is available (source checkouts/dev).
#   5. A prebuilt release binary downloaded for this platform (no toolchain).
# Step 4 precedes step 5 so a machine that can build always runs its own source;
# toolchain-less installs fall through to a download. If nothing works we print
# actionable guidance and exit non-zero.

set -o pipefail

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

is_exec() { [ -n "$1" ] && [ -x "$1" ]; }

expected_version="$(awk -F'"' '/^version[[:space:]]*=/{print $2; exit}' "$DIR/Cargo.toml")"

binary_version() {
    "$1" --version 2>/dev/null | awk '{print $NF; exit}'
}

source_is_newer() {
    find "$DIR/src" "$DIR/Cargo.toml" "$DIR/Cargo.lock" -type f -newer "$1" -print -quit 2>/dev/null |
        grep -q .
}

is_current_exec() {
    is_exec "$1" || return 1
    [ -n "$expected_version" ] || return 1
    [ "$(binary_version "$1")" = "$expected_version" ] || return 1
    ! source_is_newer "$1"
}

# 1. Explicit override.
if [ -n "${THF_BIN:-}" ]; then
    if ! is_exec "$THF_BIN"; then
        echo "history-finder: THF_BIN is not executable: $THF_BIN" >&2
        exit 1
    fi
    exec "$THF_BIN" "$@"
fi

# 2 & 3. Already-present binaries, freshest local build first, then a download.
for candidate in \
    "$DIR/target/release/thf" \
    "$DIR/target/debug/thf" \
    "$DIR/bin/thf"; do
    if is_current_exec "$candidate"; then
        exec "$candidate" "$@"
    fi
done

# 4. Source checkout with a toolchain: run (and rebuild on change) from source.
if command -v cargo >/dev/null 2>&1; then
    exec cargo run --quiet --manifest-path "$DIR/Cargo.toml" -- "$@"
fi

# 5. No toolchain: fetch a prebuilt release binary for this platform, then exec.
if [ "${THF_AUTO_DOWNLOAD:-1}" != "0" ]; then
    install_args=()
    [ -e "$DIR/bin/thf" ] && install_args+=(--force)
    if bash "$DIR/scripts/install-binary.sh" "${install_args[@]}" >&2 &&
        is_current_exec "$DIR/bin/thf"; then
        exec "$DIR/bin/thf" "$@"
    fi
fi

echo "history-finder: could not find or obtain the 'thf' backend." >&2
echo "  - Install a Rust toolchain, then: cargo build --release" >&2
echo "  - Or download a prebuilt binary:  bash '$DIR/scripts/install-binary.sh'" >&2
echo "  - Or point THF_BIN at an existing 'thf' executable." >&2
exit 1

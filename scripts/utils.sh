#!/usr/bin/env bash
# Deprecated compatibility shim. Shared logic now lives in the Rust thf binary.

thf_shell_quote() {
    printf '%q' "$1"
}

thf_version() {
    local current_dir
    current_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
    bash "$current_dir/../history_finder.sh" --version | awk '{print $2}'
}

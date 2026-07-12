#!/usr/bin/env bash
# Deprecated compatibility shim. Shared logic now lives in the Rust tnx binary.

tnx_shell_quote() {
    printf '%q' "$1"
}

tnx_version() {
    local current_dir
    current_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
    bash "$current_dir/../tnx" --version | awk '{print $2}'
}

#!/usr/bin/env bash
# Compatibility wrapper. The implementation lives in the Rust thf binary.

set -o pipefail

CURRENT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
exec bash "$CURRENT_DIR/../history_finder.sh" search "$@"

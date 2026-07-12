#!/usr/bin/env bash
# Compatibility wrapper. The implementation lives in the Rust tnx binary.

set -o pipefail

CURRENT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
exec bash "$CURRENT_DIR/../tnx" search "$@"

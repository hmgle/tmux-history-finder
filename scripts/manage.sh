#!/usr/bin/env bash
# Compatibility wrapper for direct manager category/action bindings.

set -o pipefail

CURRENT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
exec bash "$CURRENT_DIR/../tnx" manage "$@"

#!/usr/bin/env bash
# Compatibility wrapper for the legacy capture.sh path.

set -o pipefail

CURRENT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
exec bash "$CURRENT_DIR/../tnx" capture "$@"

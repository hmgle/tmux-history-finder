#!/usr/bin/env bash
# Compatibility wrapper for the legacy preview.sh path.

set -o pipefail

CURRENT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
exec bash "$CURRENT_DIR/../history_finder.sh" preview "$@"

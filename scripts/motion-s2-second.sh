#!/usr/bin/env bash
# Prompt for the second motion character after motion-s2.sh stores the first.

set -o pipefail

CURRENT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
client_pid="${1:-}"
target_window="${2:-}"

if [ -z "$client_pid" ] || [ -z "$target_window" ]; then
    exit 0
fi

query_option="@tmux_history_finder_motion_query_$client_pid"
first=$(tmux show-option -gqv "$query_option" 2>/dev/null)

if [ -z "$first" ]; then
    exit 0
fi

tmux command-prompt -1F -p "motion 2/2: $first" \
    "set-option -gq $query_option '$first%%%'; run-shell '$CURRENT_DIR/scripts/motion-run.sh s2 $client_pid $target_window'"

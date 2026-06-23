#!/usr/bin/env bash
# Combine prompted motion characters and launch two-character motion mode.

set -o pipefail

CURRENT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
client_pid="${1:-}"
target_window="${2:-}"

if [ -z "$client_pid" ] || [ -z "$target_window" ]; then
    exit 0
fi

query_option="@tmux_history_finder_motion_query_$client_pid"
second_option="@tmux_history_finder_motion_query_second_$client_pid"
first=$(tmux show-option -gqv "$query_option" 2>/dev/null)
second=$(tmux show-option -gqv "$second_option" 2>/dev/null)

tmux set-option -gu "$second_option" 2>/dev/null || :

if [ -z "$first" ] || [ -z "$second" ]; then
    exit 0
fi

tmux set-option -gq "$query_option" "$first$second"
"$CURRENT_DIR/scripts/motion-run.sh" s2 "$client_pid" "$target_window"

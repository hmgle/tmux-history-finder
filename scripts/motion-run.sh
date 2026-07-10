#!/usr/bin/env bash
# Launch motion mode for the originating tmux client.

set -o pipefail

CURRENT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
kind="${1:-}"
client_pid="${2:-}"
target_window="${3:-}"
target_client="${4:-}"

if [ -z "$kind" ] || [ -z "$client_pid" ] || [ -z "$target_window" ]; then
    exit 0
fi

case "$kind" in
    s|s2) ;;
    *) exit 0 ;;
esac

query_option="@tmux_history_finder_motion_query_$client_pid"
if [ -z "$target_client" ]; then
    target_client=$(tmux list-clients -F '#{client_pid}	#{client_name}' 2>/dev/null |
        awk -F '\t' -v pid="$client_pid" '$1 == pid { print $2; exit }')
fi

cmd=(
    "$CURRENT_DIR/history_finder.sh"
    motion
    "$kind"
    --query-option "$query_option"
    --target-window "$target_window"
)
if [ -n "$target_client" ]; then
    cmd+=(--target-client "$target_client")
fi

exec "${cmd[@]}"

#!/usr/bin/env bash
# Launch motion mode in a temporary tmux window.

set -o pipefail

CURRENT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
kind="${1:-}"
client_pid="${2:-}"
target_window="${3:-}"

if [ -z "$kind" ] || [ -z "$client_pid" ] || [ -z "$target_window" ]; then
    exit 0
fi

case "$kind" in
    s|s2) ;;
    *) exit 0 ;;
esac

shell_quote() {
    case "$1" in
        (*[!A-Za-z0-9_./:@%+=-]*|'')
            printf "'%s'" "$(printf "%s" "$1" | sed "s/'/'\"'\"'/g")"
            ;;
        (*)
            printf "%s" "$1"
            ;;
    esac
}

query_option="@tmux_history_finder_motion_query_$client_pid"
cmd="$(shell_quote "$CURRENT_DIR/history_finder.sh") motion $(shell_quote "$kind") --query-option $(shell_quote "$query_option") --target-window $(shell_quote "$target_window")"

tmux new-window -d "$cmd"

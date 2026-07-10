#!/usr/bin/env bash
# Prompt for two characters and launch tmux-history-finder motion mode.

CURRENT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=scripts/utils.sh
source "$CURRENT_DIR/scripts/utils.sh"

client_pid="${1:-}"
target_window="${2:-}"
target_client="${3:-}"
[ -n "$client_pid" ] && [ -n "$target_window" ] || exit 0

query_option="@tmux_history_finder_motion_query_$client_pid"
second_command="$(thf_shell_quote "$CURRENT_DIR/scripts/motion-s2-second.sh") $(thf_shell_quote "$client_pid") $(thf_shell_quote "$target_window")"
[ -z "$target_client" ] || second_command="$second_command $(thf_shell_quote "$target_client")"

tmux command-prompt -1F -p "motion 1/2:" \
    "set-option -gq $query_option '%%%'" \
    \; run-shell "$second_command"

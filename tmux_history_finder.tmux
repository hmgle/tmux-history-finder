#!/usr/bin/env bash
# tmux-history-finder plugin entry point.

CURRENT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

launch_key=$(tmux show-option -gqv "@tmux_history_finder_launch_key" 2>/dev/null)
launch_key="${launch_key:-g}"
prompt_query=$(tmux show-option -gqv "@tmux_history_finder_prompt_query" 2>/dev/null)
prompt_query="${prompt_query:-${THF_PROMPT_QUERY:-0}}"

case "$prompt_query" in
    1|true|yes|on)
        tmux bind-key "$launch_key" command-prompt -F -p "history search:" -T search \
            "set-option -gq @tmux_history_finder_last_query_#{client_pid} '%%%'; run-shell -b \"$CURRENT_DIR/history_finder.sh search --query-option @tmux_history_finder_last_query_#{client_pid} --require-query\""
        ;;
    *)
        tmux bind-key "$launch_key" run-shell -b "$CURRENT_DIR/history_finder.sh search"
        ;;
esac

if [ -z "$(tmux show-option -gqv "@thf_loaded")" ]; then
    tmux set-option -g "@thf_loaded" "1"
    tmux display-message "tmux-history-finder loaded: press Prefix+$launch_key to search panes" 2>/dev/null || :
fi

#!/usr/bin/env bash
# tmux-history-finder plugin entry point.

CURRENT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

launch_key=$(tmux show-option -gqv "@tmux_history_finder_launch_key" 2>/dev/null)
launch_key="${launch_key:-g}"

tmux bind-key "$launch_key" run-shell -b "$CURRENT_DIR/history_finder.sh search"

if [ -z "$(tmux show-option -gqv "@thf_loaded")" ]; then
    tmux set-option -g "@thf_loaded" "1"
    tmux display-message "tmux-history-finder loaded: press Prefix+$launch_key to search panes" 2>/dev/null || :
fi

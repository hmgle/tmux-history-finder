#!/usr/bin/env bash
# Prompt for two characters and launch tmux-history-finder motion mode.

CURRENT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

tmux command-prompt -1F -p "motion 1/2:" \
    "set-option -gq @tmux_history_finder_motion_query_#{client_pid} '%%%'; run-shell '$CURRENT_DIR/scripts/motion-s2-second.sh #{client_pid} #{window_id}'"

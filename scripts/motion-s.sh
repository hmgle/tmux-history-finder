#!/usr/bin/env bash
# Prompt for one character and launch tmux-history-finder motion mode.

CURRENT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

tmux command-prompt -1F -p "motion:" \
    "set-option -gq @tmux_history_finder_motion_query_#{client_pid} '%%%'; run-shell '$CURRENT_DIR/scripts/motion-run.sh s #{client_pid} #{window_id}'"

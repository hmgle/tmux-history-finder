#!/usr/bin/env bash
# tmux-history-finder plugin entry point.

CURRENT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

launch_key=$(tmux show-option -gqv "@tmux_history_finder_launch_key" 2>/dev/null)
launch_key="${launch_key:-g}"
motion_key=$(tmux show-option -gqv "@tmux_history_finder_motion_key" 2>/dev/null)
motion_key="${motion_key:-s}"
motion2_key=$(tmux show-option -gqv "@tmux_history_finder_motion2_key" 2>/dev/null)
motion_copy_mode_no_prefix=$(tmux show-option -gqv "@tmux_history_finder_motion_copy_mode_no_prefix" 2>/dev/null)
motion_copy_mode_no_prefix="${motion_copy_mode_no_prefix:-${THF_MOTION_COPY_MODE_NO_PREFIX:-0}}"
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

if [ -n "$motion_key" ]; then
    tmux bind-key "$motion_key" run-shell "$CURRENT_DIR/scripts/motion-s.sh"
fi

if [ -n "$motion2_key" ]; then
    tmux bind-key "$motion2_key" run-shell "$CURRENT_DIR/scripts/motion-s2.sh"
fi

case "$motion_copy_mode_no_prefix" in
    1|true|yes|on)
        mode_keys=$(tmux show-option -gqv mode-keys 2>/dev/null)
        if [ "$mode_keys" = "vi" ]; then
            copy_mode_table="copy-mode-vi"
        else
            copy_mode_table="copy-mode"
        fi
        if [ -n "$motion_key" ]; then
            tmux bind-key -T "$copy_mode_table" "$motion_key" run-shell "$CURRENT_DIR/scripts/motion-s.sh"
        fi
        if [ -n "$motion2_key" ]; then
            tmux bind-key -T "$copy_mode_table" "$motion2_key" run-shell "$CURRENT_DIR/scripts/motion-s2.sh"
        fi
        ;;
esac

if [ -z "$(tmux show-option -gqv "@thf_loaded")" ]; then
    tmux set-option -g "@thf_loaded" "1"
    tmux display-message "tmux-history-finder loaded: Prefix+$launch_key search, Prefix+$motion_key motion" 2>/dev/null || :
fi

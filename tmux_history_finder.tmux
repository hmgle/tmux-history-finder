#!/usr/bin/env bash
# tmux-history-finder plugin entry point.
#
# This file is executed by tmux (via TPM's `run-shell`, or `run` in tmux.conf)
# when the plugin loads. It wires up the default key binding that launches the
# interactive search.
#
# Configuration is read from tmux @tmux_history_finder_* options. We deliberately
# do NOT import them here: this load-time shell exits immediately, so anything it
# exported would be gone before the key binding spawns its own run-shell process
# later. Instead scripts/utils.sh reads the @-options at run time (and applies
# THF_* defaults), which works for both the key binding and the standalone CLI.

CURRENT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# We only need the launch key here, to register the binding.
launch_key=$(tmux show-option -gqv "@tmux_history_finder_launch_key" 2>/dev/null)
launch_key="${launch_key:-g}"

# Bind <prefix> + $launch_key to open the picker. run-shell runs the script in
# the background so the tmux client stays responsive; the spawned process
# inherits the tmux server environment and reads its config on startup.
tmux bind-key "$launch_key" run-shell -b "$CURRENT_DIR/scripts/search.sh"

# Friendly notice on first load (only shown if the user is looking).
if ! tmux show-option -gqv "@thf_loaded" >/dev/null 2>&1; then
    tmux set-option -g "@thf_loaded" "1"
    tmux display-message "tmux-history-finder loaded: press Prefix+$launch_key to search panes" 2>/dev/null || :
fi

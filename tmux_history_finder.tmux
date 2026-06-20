#!/usr/bin/env bash
# tmux-history-finder plugin entry point.
#
# This file is executed by tmux (via TPM's `run-shell`, or `run` in tmux.conf)
# when the plugin loads. It wires up the default key binding that launches the
# interactive search.
#
# Configuration is read from tmux @tmux_history_finder_* options and exported
# as THF_* environment variables for the helper scripts. Each option has a
# sensible default applied in scripts/utils.sh.

CURRENT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# --- Pull user options from tmux @-options into the environment --------------
# We map @tmux_history_finder_<name> -> THF_<NAME> so the scripts see one config
# surface regardless of how the value was set. tmux @-options are the idiomatic
# TPM config style; the environment remains a fallback for CLI use.
import_option() {
    local name="$1" env_name="$2" value
    value=$(tmux show-option -gqv "@tmux_history_finder_${name}" 2>/dev/null)
    [ -n "$value" ] && export "$env_name=$value"
}

import_option launch_key     THF_LAUNCH_KEY
import_option scope          THF_SCOPE
import_option include_history THF_INCLUDE_HISTORY
import_option case           THF_CASE
import_option backend        THF_BACKEND
import_option join_wraps     THF_JOIN_WRAPS
import_option skip_blank     THF_SKIP_BLANK
import_option preview        THF_PREVIEW
import_option default_action THF_DEFAULT_ACTION
import_option fzf_options    THF_FZF_OPTIONS

# Export the plugin dir so launched scripts can resolve their own location even
# when run via run-shell from an unknown cwd.
export THF_PLUGIN_DIR="$CURRENT_DIR"

launch_key="${THF_LAUNCH_KEY:-g}"

# Bind <prefix> + $launch_key to open the picker. run-shell runs the script in
# the background so the tmux client stays responsive; we pass the socket context
# implicitly because we run inside tmux.
tmux bind-key "$launch_key" run-shell -b "$CURRENT_DIR/scripts/search.sh"
# A second binding with the leader-less form for convenience (Prefix-g is the
# documented default). Users can rebind freely.

# Friendly notice on first load (only shown if the user is looking).
if ! tmux show-option -gqv "@thf_loaded" >/dev/null 2>&1; then
    tmux set-option -g "@thf_loaded" "1"
    tmux display-message "tmux-history-finder loaded: press Prefix+$launch_key to search panes" 2>/dev/null || :
fi

#!/usr/bin/env bash
# history_finder.sh — command-line entry point for tmux-history-finder.
#
# This is what you run directly (or symlink onto $PATH) when you want to search
# pane content without the tmux key binding, e.g. from a shell prompt or a
# script. All search.sh options pass straight through.
#
# Examples:
#   history_finder.sh                       # interactive picker, all panes
#   history_finder.sh 'error'               # pre-filtered to lines matching 'error'
#   history_finder.sh --scope session foo   # current session only
#   history_finder.sh --print 'panic'       # print matching lines (no UI), scriptable
#   history_finder.sh --action copy 'token' # copy the selected line to clipboard
#
# Works both inside tmux (talks to the current server) and outside (talks to the
# server named by $TMUX, or you can pass a socket via THF_TMUX_ARGS).

set -o pipefail

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Require tmux to exist; without it there's nothing to capture.
if ! command -v tmux >/dev/null 2>&1; then
    echo "history-finder: tmux is not installed or not in PATH." >&2
    exit 1
fi

# Hand off to the interactive search. search.sh handles all option parsing and
# the fzf UI; this wrapper just provides a stable, memorable command name and
# the presence/dependency checks.
exec bash "$DIR/scripts/search.sh" "$@"

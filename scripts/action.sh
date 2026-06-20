#!/usr/bin/env bash
# Act on a single search result record.
#
# Consumed record format (TAB-separated, produced by capture.sh):
#   pane_id \t location \t command \t window_name \t line_no \t text
#
# Actions:
#   jump   Switch the client to the result's pane, enter copy-mode and run
#          search-forward so the cursor lands on the matched text. This is more
#          robust than goto-line: it finds the exact text even when our logical
#          line number drifts from tmux's buffer coordinate (e.g. because of
#          blank-line skipping or wrap joining).
#   copy   Put the matched line's text into the tmux paste buffer (and the
#          system clipboard if a clipboard helper is available).
#   send   Type the matched line's text into the *current* pane's active program.
#   print  Write the matched line's text to stdout (useful for scripting).

set -o pipefail

CURRENT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=utils.sh
. "$CURRENT_DIR/utils.sh"

action="${THF_DEFAULT_ACTION:-jump}"
record=""

while [ $# -gt 0 ]; do
    case "$1" in
        --action) action="$2"; shift 2 ;;
        --record) record="$2"; shift 2 ;;
        -h|--help) sed -n '2,17p' "$0"; exit 0 ;;
        *) echo "action.sh: unexpected argument: $1" >&2; exit 2 ;;
    esac
done

if [ -z "$record" ]; then
    echo "action.sh: --record is required" >&2
    exit 2
fi

# Split the record into its fields. We read into 5 named vars and let the rest
# (the text, which may itself contain tabs) fall into `text`.
# shellcheck disable=SC2034  # command/window_name are parsed for clarity, unused here
IFS=$'\t' read -r pane_id location command window_name line_no text <<EOF
$record
EOF

[ -z "$pane_id" ] && { echo "action.sh: malformed record (no pane_id)" >&2; exit 2; }

# Strip a trailing carriage return if capture-pane left one (it can on some
# platforms when a line was overwritten by a CR-moving cursor).
text=${text%$'\r'}

case "$action" in
    jump)
        # Bring the user to the pane, then reveal the match in copy-mode.
        # switch-client + select-window + select-pane so we focus it regardless
        # of which session/window it lives in.
        ses=${location%%:*}        # session name (before first ':')
        winloc=${location#*:}      # window.pane
        win=${winloc%%.*}
        thf_tmux switch-client -t "$ses" 2>/dev/null || :
        thf_tmux select-window -t "$ses:$win" 2>/dev/null || :
        thf_tmux select-pane -t "$pane_id" 2>/dev/null || :

        # Enter copy-mode fresh (idempotent: if already in a mode, top clears
        # any prior search/cursor so our search-forward is unambiguous).
        thf_tmux copy-mode -t "$pane_id" 2>/dev/null || :
        # Start from the top of history so search-forward scans everything and
        # lands on the first (oldest) occurrence, matching the index order.
        thf_tmux send-keys -t "$pane_id" -X history-top 2>/dev/null || :

        # Prefer an exact search on the text. tmux's search-forward treats the
        # argument as a regular expression, so we escape metacharacters to get a
        # literal match. We use a distinctive prefix (trimmed, capped) to be
        # tolerant of trailing whitespace or truncation in the captured text.
        needle=$(printf '%s' "$text" | sed -E 's/^[[:space:]]+//' | cut -c1-80)
        if [ -n "$needle" ]; then
            needle=$(thf_regex_escape "$needle")
            thf_tmux send-keys -t "$pane_id" -X search-forward "$needle" 2>/dev/null || :
        else
            # Blank text: fall back to the recorded line number.
            thf_tmux send-keys -t "$pane_id" -X goto-line "$line_no" 2>/dev/null || :
        fi
        ;;

    copy)
        # tmux paste buffer first (so tmux's own paste works), then system clip.
        thf_tmux set-buffer -- "$text" 2>/dev/null || :
        clip=$(thf_clip_cmd)
        if [ -n "$clip" ]; then
            printf '%s' "$text" | $clip 2>/dev/null || :
        else
            # No system clipboard helper: keep it in the tmux buffer and tell the user.
            thf_tmux display-message "tmux-history-finder: copied to tmux buffer (no system clipboard found)" 2>/dev/null || :
        fi
        ;;

    send)
        # Send the matched text as keypresses to the active pane. We target the
        # *current* pane (the one the user is in), not the source pane, since
        # "send" is meant to reuse found text as input.
        thf_tmux send-keys -l -- "$text" 2>/dev/null || :
        ;;

    print)
        printf '%s\n' "$text"
        ;;

    *)
        echo "action.sh: unknown action '$action' (expected jump|copy|send|print)" >&2
        exit 2
        ;;
esac

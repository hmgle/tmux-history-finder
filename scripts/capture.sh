#!/usr/bin/env bash
# Build the searchable index of every pane's (history + visible) content.
#
# Output format (one record per non-skipped line, TAB-separated):
#   pane_id \t location \t command \t window_name \t line_no \t text
#
#   location   = session:window.pane   (e.g. main:1.0)
#   line_no    = 1-based logical line number inside the pane's full history,
#                matching the coordinate system used by `copy-mode` goto-line.
#
# Usage:
#   capture.sh [output_file]            # default scope/history from config
#   capture.sh -s all|session|pane [output_file]
#   capture.sh --no-history             # visible screen only
#   capture.sh -t <pane_id>             # restrict to a single pane
#
# When no output_file is given the index is written to stdout.

set -o pipefail

CURRENT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=utils.sh
. "$CURRENT_DIR/utils.sh"

scope="$THF_SCOPE"
include_history="$THF_INCLUDE_HISTORY"
join_wraps="$THF_JOIN_WRAPS"
skip_blank="$THF_SKIP_BLANK"
target_pane=""
out=""

while [ $# -gt 0 ]; do
    case "$1" in
        -s|--scope)         scope="$2"; shift 2 ;;
        --no-history)       include_history=0; shift ;;
        --history)          include_history=1; shift ;;
        --no-join)          join_wraps=0; shift ;;
        --no-skip-blank)    skip_blank=0; shift ;;
        -t|--target)        target_pane="$2"; shift 2 ;;
        -o|--output)        out="$2"; shift 2 ;;
        -h|--help)
            sed -n '2,20p' "$0"
            exit 0 ;;
        *)
            if [ -z "$out" ]; then out="$1"; else
                echo "capture.sh: unexpected argument: $1" >&2; exit 2
            fi
            shift ;;
    esac
done

# `capture-pane` line-range flags.
#   -S - -E -   => whole history through last line
#   (omitted)   => only the currently visible screen
cap_flags="-p"
[ "$join_wraps" = 1 ] && cap_flags="$cap_flags -J"
if [ "$include_history" = 1 ]; then
    cap_flags="$cap_flags -S - -E -"
fi

# Choose the pane list depending on scope. We always carry pane_id so the
# result stays unambiguous even across sessions with identical window names.
list_panes() {
    # NOTE: tmux's -F format does NOT translate the two characters "\t" into a
    # tab -- it emits them literally. We must embed real tab characters, so the
    # format is built with an actual tab inside double quotes (via $'\t').
    local fmt="#{session_name}$T_TB#{window_index}$T_TB#{pane_index}$T_TB#{pane_id}$T_TB#{pane_current_command}$T_TB#{window_name}"
    case "$scope" in
        pane)
            # Just the active pane of the current client (falls back to $TMUX_PANE).
            local p
            p="${TMUX_PANE:-}"
            if [ -z "$p" ]; then
                p=$(thf_tmux display-message -p '#{pane_id}' 2>/dev/null)
            fi
            [ -n "$p" ] && thf_tmux list-panes -a -F "$fmt" -f "#{==:#{pane_id},$p}" 2>/dev/null
            ;;
        session|current)
            thf_tmux list-panes -s -F "$fmt" 2>/dev/null
            ;;
        all|*)
            thf_tmux list-panes -a -F "$fmt" 2>/dev/null
            ;;
    esac
}

# Emit the index. We write to a temp file first (so callers get a stable path
# even when streaming through fzf), unless an explicit output target was given.
do_capture() {
    local raw
    raw=$(mktemp -t thf_panes.XXXXXX)
    list_panes > "$raw"

    # Build one awk process per pane. Each pane's lines get a contiguous,
    # 1-based logical line number over its non-skipped content so that the
    # number we show matches `copy-mode`'s goto-line argument.
    while IFS=$'\t' read -r ses win idx pid cmd wname; do
        [ -z "$pid" ] && continue
        [ -n "$target_pane" ] && [ "$target_pane" != "$pid" ] && continue

        # Skip dead panes only if capture would fail; capture-pane still works
        # on the last screen of a dead pane, so we keep them.

        local loc="${ses}:${win}.${idx}"
        local awk_skip
        if [ "$skip_blank" = 1 ]; then
            awk_skip='NF'
        else
            awk_skip='1'
        fi

        # NOTE: we deliberately do NOT use capture-pane -e (escape sequences),
        # so matchable text is plain and search results are clean.
        # shellcheck disable=SC2086  # $cap_flags must word-split into separate args
        thf_tmux capture-pane -t "$pid" $cap_flags 2>/dev/null \
            | awk -v OFS='\t' -v pid="$pid" -v loc="$loc" -v cmd="$cmd" \
                  -v win="$wname" '
                '"$awk_skip"' { n++; print pid, loc, cmd, win, n, $0 }
            '
    done < "$raw"

    rm -f "$raw"
}

if [ -n "$out" ]; then
    do_capture > "$out"
else
    do_capture
fi

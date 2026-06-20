#!/usr/bin/env bash
# fzf preview command. Receives the currently-highlighted record (one TAB-
# separated line from capture.sh) on argv[1] and prints a short excerpt of the
# source pane centred on the matched line.
#
# Record: pane_id \t location \t command \t window_name \t line_no \t text
#
# We re-capture the pane's history (plain text, no escape sequences) and show a
# small window of lines around the match. The matched line is located by
# searching for its text rather than by trusting the recorded line_no: the
# index's line number uses a different coordinate system than the raw buffer
# (it skips blanks and/or joins wrapped lines), so a numeric lookup would land
# on the wrong row. Searching the text is exact and self-correcting.

CURRENT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=utils.sh
. "$CURRENT_DIR/utils.sh"

record="$1"

# Parse fields: read into 5 vars, rest goes to text (may contain tabs).
IFS=$'\t' read -r pane_id location command window_name line_no text <<EOF
$record
EOF

[ -z "$pane_id" ] && { echo "(no record)"; exit 0; }

# Re-capture this pane's full history as plain text (no escape sequences).
content=$(thf_tmux capture-pane -p -S - -E - -t "$pane_id" 2>/dev/null)
[ -z "$content" ] && { echo "(could not capture $pane_id)"; exit 0; }

total=$(printf '%s\n' "$content" | wc -l | tr -d ' ')

# Locate the matching line in the raw buffer by its text. We match on a
# distinctive prefix (trimmed, capped) to be tolerant of trailing whitespace or
# minor capture differences. Use a literal fixed-string match (no regex).
needle=$(printf '%s' "$text" | sed -E 's/^[[:space:]]+//' | cut -c1-60)
# Fall back to the recorded line number if the text is empty/unmatchable.
match_line=$line_no
if [ -n "$needle" ]; then
    found=$(printf '%s\n' "$content" \
        | grep -nF -- "$needle" 2>/dev/null | head -1 | cut -d: -f1)
    [ -n "$found" ] && match_line=$found
fi
[ "$match_line" -lt 1 ] && match_line=1
[ "$match_line" -gt "$total" ] && match_line=$total

# Window of ~20 lines around the match, clamped to the buffer bounds.
half=10
start=$(( match_line - half ))
[ "$start" -lt 1 ] && start=1
end=$(( match_line + half ))
[ "$end" -gt "$total" ] && end=$total

# Header: where this comes from.
printf '\033[1;36m%s\033[0m  \033[2m(%s)\033[0m\n' "$location" "$command"
printf '\033[2mlines %d-%d of %d\033[0m\n\n' "$start" "$end" "$total"

# Print the window with line numbers; highlight the matched line.
printf '%s\n' "$content" | awk -v target="$match_line" -v start="$start" -v end="$end" '
    NR >= start && NR <= end {
        if (NR == target) printf "\033[1;33m>%6d \033[1;37m%s\033[0m\n", NR, $0
        else              printf " %6d  %s\n", NR, $0
    }
'

#!/usr/bin/env bash
# Interactive search across all (or a subset of) tmux panes.
#
# Builds the capture index (see capture.sh), runs it through fzf so the user
# can narrow results incrementally, then hands each selected record to the
# configured action (jump | copy | send | print).
#
# Record format consumed here (TAB-separated, produced by capture.sh):
#   pane_id \t location \t command \t window_name \t line_no \t text
#
# Usage:
#   search.sh [query]            # use config defaults
#   search.sh --query 'foo'
#   search.sh --scope all|session|pane
#   search.sh --action jump|copy|send|print
#   search.sh --no-history       # visible screen only
#   search.sh --no-join          # do not join wrapped lines
#   search.sh --case smart|sensitive|insensitive

set -o pipefail

CURRENT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=utils.sh
. "$CURRENT_DIR/utils.sh"

query=""

while [ $# -gt 0 ]; do
    case "$1" in
        -q|--query)         query="$2"; shift 2 ;;
        -s|--scope)         THF_SCOPE="$2"; shift 2 ;;
        --action)           THF_DEFAULT_ACTION="$2"; shift 2 ;;
        --case)             THF_CASE="$2"; shift 2 ;;
        --no-history)       THF_INCLUDE_HISTORY=0; shift ;;
        --history)          THF_INCLUDE_HISTORY=1; shift ;;
        --no-join)          THF_JOIN_WRAPS=0; shift ;;
        --no-skip-blank)    THF_SKIP_BLANK=0; shift ;;
        -t|--target)        THF_TARGET_PANE="$2"; shift 2 ;;
        --print)            THF_DEFAULT_ACTION=print; shift ;;
        -h|--help)
            sed -n '2,22p' "$0"
            exit 0 ;;
        -*)
            echo "search.sh: unknown option: $1" >&2; exit 2 ;;
        *)
            if [ -z "$query" ]; then query="$1"; else
                echo "search.sh: unexpected argument: $1" >&2; exit 2
            fi
            shift ;;
    esac
done

# `--print` with a query runs non-interactively: we pre-filter the index and
# emit the matched text straight to stdout, no picker. Detect that here so we
# can skip the fzf requirement below (fzf is only needed for the interactive UI).
noninteractive=0
[ "$THF_DEFAULT_ACTION" = print ] && [ -n "$query" ] && noninteractive=1

# Need fzf for the interactive picker. In non-interactive `--print` mode we never
# launch it, so the requirement only applies to the interactive path.
if [ "$noninteractive" = 0 ] && ! thf_have fzf; then
    echo "tmux-history-finder: fzf is required but was not found in PATH." >&2
    exit 1
fi

# --- Build the index ----------------------------------------------------------
# We let capture.sh write to a temp file so fzf can re-read it (and so the
# preview command can re-open it to show surrounding lines).
index_file=$(mktemp -t thf_index.XXXXXX)
search_input=""
preview_dir=""
# One trap cleans up every temp path. The ${var:+...} guards skip the ones not
# created yet (search_input and preview_dir are filled in below).
trap 'rm -rf "$index_file" ${search_input:+"$search_input"} ${preview_dir:+"$preview_dir"}' EXIT

cap_args=(--output "$index_file")
[ -n "${THF_TARGET_PANE:-}" ] && cap_args+=(-t "$THF_TARGET_PANE")

# shellcheck disable=SC2153
THF_SCOPE="$THF_SCOPE" THF_INCLUDE_HISTORY="$THF_INCLUDE_HISTORY" \
THF_JOIN_WRAPS="$THF_JOIN_WRAPS" THF_SKIP_BLANK="$THF_SKIP_BLANK" \
    bash "$CURRENT_DIR/capture.sh" "${cap_args[@]}" 2>/dev/null

if [ ! -s "$index_file" ]; then
    thf_tmux display-message "tmux-history-finder: no pane content to search" 2>/dev/null \
        || echo "tmux-history-finder: no pane content to search" >&2
    exit 0
fi

# --- Pre-filter with the search backend when a query is given -----------------
# Feeding fzf a pre-narrowed list keeps huge histories snappy and makes the
# "Enter jumps" path land on the first real hit immediately. When no query is
# given we hand fzf the whole index and let it do the narrowing.
search_input=$(mktemp -t thf_filter.XXXXXX)

if [ -n "$query" ]; then
    backend=$(thf_resolve_backend)
    # Single source of truth for case handling (see thf_case_flags). The query is
    # passed so grep's smart-case emulation can inspect it for an uppercase letter.
    case_flags=$(thf_case_flags "$THF_CASE" "$backend" "$query")
    # Pre-filter the index down to matching records, passing the full record
    # through untouched so the action handler still gets all six fields.
    if [ "$backend" = rg ]; then
        # --no-config: ignore a user's ~/.ripgreprc so behavior is predictable.
        # No -n: a line-number prefix would corrupt the TAB fields.
        # shellcheck disable=SC2086
        rg --no-config $case_flags -- "$query" "$index_file" \
            > "$search_input" 2>/dev/null || :
    else
        # shellcheck disable=SC2086
        grep -E $case_flags -- "$query" "$index_file" \
            > "$search_input" 2>/dev/null || :
    fi
else
    cp "$index_file" "$search_input"
fi

if [ ! -s "$search_input" ]; then
    msg="tmux-history-finder: no matches"
    [ -n "$query" ] && msg="$msg for '$query'"
    thf_tmux display-message "$msg" 2>/dev/null || echo "$msg" >&2
    exit 0
fi

# --- Non-interactive print ----------------------------------------------------
# `--print 'query'` skips the picker entirely and writes each matching line's
# text to stdout (scriptable, no UI). We reuse action.sh's print so the field
# parsing stays in one place.
if [ "$noninteractive" = 1 ]; then
    while IFS= read -r line; do
        [ -z "$line" ] && continue
        bash "$CURRENT_DIR/action.sh" --action print --record "$line"
    done < "$search_input"
    exit 0
fi

# --- fzf options --------------------------------------------------------------
# Display: location : command : window : line : text  (hide pane_id + line dup).
# We keep pane_id and line_no in the selected output (fields 1 and 5) for the
# action handler, but hide them from the list with --with-nth.
header=$(thf_build_header)

# shellcheck disable=SC2054  # commas inside "2,3,4,5,6" are fzf syntax, not separators
fzf_opts_common=(
    --delimiter "$T_TB"
    --with-nth 2,3,4,5,6
    --ansi
    --layout=reverse
    --info=inline
    --prompt 'history> '
    --header "$header"
    --multi
    --tiebreak=index
)
# Preview shows the source pane scrolled to the matching line. We hand the
# preview command a private cache dir (cleaned up by the trap above) so it can
# capture each pane once and reuse it instead of re-capturing the whole
# scrollback on every fzf navigation. Honour THF_PREVIEW so it can be disabled.
if [ "${THF_PREVIEW:-1}" = 1 ]; then
    preview_dir=$(mktemp -d -t thf_prev.XXXXXX)
    export THF_PREVIEW_CACHE_DIR="$preview_dir"
    fzf_opts_common+=(--preview-window right:60%:wrap --preview "$CURRENT_DIR/preview.sh {}")
fi
# A pre-entered query makes fzf land on the first hit immediately. We don't use
# --exit-0: even with a pre-filtered list, we want the user to be able to refine
# the query interactively rather than have fzf bail out on no match.
[ -n "$query" ] && fzf_opts_common+=(--query "$query")

# Honour any user override without clobbering our defaults. Split on whitespace
# into an array so quoted values with spaces are preserved per-token (this is a
# best-effort escape hatch; users wanting spaces within a single option should
# wrap the whole THF_FZF_OPTIONS value accordingly).
if [ -n "${THF_FZF_OPTIONS:-}" ]; then
    # shellcheck disable=SC2206
    fzf_opts_common+=($THF_FZF_OPTIONS)
fi

# fzf reads the pre-filtered records from stdin, writes the user's selection(s)
# to stdout. fzf-tmux / popup routing is handled by thf_fzf_invoke.
selection=$(thf_fzf_invoke "${fzf_opts_common[@]}" < "$search_input")

# ESC / empty selection -> nothing to do.
[ -z "$selection" ] && exit 0

# --- Dispatch each selected record to the action handler ----------------------
# Multiple selections are dispatched one per line; jump/copy/send/print all
# operate per-record. We pass the raw record (all 6 fields) through.
printf '%s\n' "$selection" | while IFS= read -r line; do
    [ -z "$line" ] && continue
    bash "$CURRENT_DIR/action.sh" --action "$THF_DEFAULT_ACTION" --record "$line"
done

#!/usr/bin/env bash
# Shared configuration, capability detection and helper functions for
# tmux-history-finder. Sourced by the other scripts; not meant to be run directly.

# Resolve the directory that contains this file so the scripts work no matter
# the current working directory (also when invoked through `run-shell`).
# shellcheck disable=SC2034
THF_CURRENT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]:-$0}")" && pwd)"
# The plugin root is one level up from scripts/.
# shellcheck disable=SC2034
THF_PLUGIN_DIR="$(cd "$THF_CURRENT_DIR/.." && pwd)"

# A real tab character. tmux's -F format emits the two characters "\t" verbatim
# rather than translating them to a tab, so any -F format that needs field
# separation must embed a genuine tab. All scripts source this constant.
# shellcheck disable=SC2155
export T_TB=$'\t'

thf_version() { echo "0.1.0"; }

# --- Capability detection -----------------------------------------------------

thf_have() { command -v "$1" >/dev/null 2>&1; }

# Run a tmux command, honouring an explicit socket override if provided.
thf_tmux() {
    if [ -n "${THF_TMUX_ARGS:-}" ]; then
        # shellcheck disable=SC2086
        tmux $THF_TMUX_ARGS "$@"
    else
        tmux "$@"
    fi
}

# --- Configuration (THF_*) ---------------------------------------------------
# Each option has a sensible default that can be overridden via the environment
# or via a user's tmux.conf (set-environment).

# Import @tmux_history_finder_<name> options into THF_* variables, but only
# where the variable isn't already set, so an explicit environment value or a
# CLI flag still wins. This is the single place configuration is resolved, so
# the key binding and the CLI behave identically. Guarded by THF_OPTIONS_IMPORTED
# so child processes (the fzf preview, the capture/action helpers) reuse the
# resolved values instead of re-running `tmux show-option` on every invocation.
thf_import_options() {
    [ -n "${THF_OPTIONS_IMPORTED:-}" ] && return 0
    thf_have tmux || return 0
    local pair name env_name cur value
    for pair in \
        launch_key:THF_LAUNCH_KEY scope:THF_SCOPE \
        include_history:THF_INCLUDE_HISTORY case:THF_CASE backend:THF_BACKEND \
        join_wraps:THF_JOIN_WRAPS skip_blank:THF_SKIP_BLANK preview:THF_PREVIEW \
        default_action:THF_DEFAULT_ACTION fzf_options:THF_FZF_OPTIONS; do
        name=${pair%%:*}
        env_name=${pair#*:}
        cur=${!env_name}                # indirect read; empty when unset
        [ -n "$cur" ] && continue
        # Read through thf_tmux so an explicit THF_TMUX_ARGS socket/server is
        # honoured here too, consistent with every other tmux call.
        value=$(thf_tmux show-option -gqv "@tmux_history_finder_${name}" 2>/dev/null) || value=""
        [ -n "$value" ] && export "$env_name=$value"
    done
}
thf_import_options

# Key binding used to launch the interactive search. Default: `g` under prefix.
: "${THF_LAUNCH_KEY:=g}"
# Search scope: all | session | pane.
: "${THF_SCOPE:=all}"
# Whether to include scrollback history (1) or only the visible screen (0).
: "${THF_INCLUDE_HISTORY:=1}"
# Case matching: smart | sensitive | insensitive.
: "${THF_CASE:=smart}"
# Search backend: auto | rg | grep.
: "${THF_BACKEND:=auto}"
# Join wrapped lines so visually-wrapped content is treated as one logical line.
: "${THF_JOIN_WRAPS:=1}"
# Drop lines that are empty/whitespace-only from the index.
: "${THF_SKIP_BLANK:=1}"
# Preview the source pane inside fzf.
: "${THF_PREVIEW:=1}"
# Extra fzf options appended to the defaults.
: "${THF_FZF_OPTIONS:=}"
# Action when pressing Enter on a result: jump | copy | send | print.
: "${THF_DEFAULT_ACTION:=jump}"
# The clipboard backend for `copy` is auto-detected (pbcopy | wl-copy | xclip |
# xsel | clip.exe); see thf_clip_cmd.

# Export the resolved configuration so child processes inherit it (and skip the
# import above). Re-assigning an exported variable keeps the export attribute,
# so CLI overrides applied later in search.sh propagate automatically.
export THF_LAUNCH_KEY THF_SCOPE THF_INCLUDE_HISTORY THF_CASE THF_BACKEND \
       THF_JOIN_WRAPS THF_SKIP_BLANK THF_PREVIEW THF_DEFAULT_ACTION \
       THF_FZF_OPTIONS THF_OPTIONS_IMPORTED=1

# --- Derived helpers ----------------------------------------------------------

# Pick a search backend respecting THF_BACKEND and what's installed.
thf_resolve_backend() {
    case "$THF_BACKEND" in
        rg)   thf_have rg && { echo rg; return; } ; echo grep ;;
        grep) echo grep ;;
        auto|*)
            if thf_have rg; then echo rg; else echo grep; fi ;;
    esac
}

# Build the case-sensitivity flag for the chosen backend, used by both the rg
# and grep pre-filters in search.sh. Empty output means "no flag"; both backends
# are case-sensitive by default.
#   args: [case] [backend] [query]
thf_case_flags() {
    local mode="${1:-$THF_CASE}" backend="${2:-$(thf_resolve_backend)}" query="${3:-}"
    case "$mode" in
        insensitive) printf '%s' '-i' ;;
        sensitive)   : ;;  # no flag needed; default is case-sensitive
        smart|*)
            if [ "$backend" = rg ]; then
                printf '%s' '-S'        # ripgrep has native smart-case
            else
                # grep has no smart-case: emulate it -- case-insensitive unless
                # the query contains an uppercase letter.
                case "$query" in
                    *[A-Z]*) : ;;       # has uppercase -> keep case-sensitive
                    *)       printf '%s' '-i' ;;
                esac
            fi ;;
    esac
}

# Locate a working clipboard copy command, or print nothing.
thf_clip_cmd() {
    for c in pbcopy wl-copy xclip xsel clip.exe; do
        if thf_have "$c"; then
            case "$c" in
                xsel) echo "xsel --clipboard --input" ;;
                *)    echo "$c" ;;
            esac
            return
        fi
    done
}

# Compare two dotted versions: prints ">" if $1 > $2 else "<=".
thf_ver_cmp() {
    local a="$1" b="$2"
    if [ "$(printf '%s\n%s\n' "$a" "$b" | sort -V | head -n1)" = "$b" ] && [ "$a" != "$b" ]; then
        echo ">"
    else
        echo "<="
    fi
}

# Decide whether the installed fzf/tmux support the popup (-p) mode.
thf_use_popup() {
    local tmux_v fzf_v
    tmux_v=$(thf_tmux -V 2>/dev/null | grep -oE '[0-9]+\.[0-9]+' | head -n1)
    [ -z "$tmux_v" ] && tmux_v=0.0
    fzf_v=$(fzf --version 2>/dev/null | grep -oE '[0-9]+\.[0-9]+' | head -n1)
    [ -z "$fzf_v" ] && fzf_v=0.0
    # tmux >= 3.2 supports display-popup; fzf >= 0.23 ships fzf-tmux popup mode.
    if [ "$(thf_ver_cmp "$tmux_v" 3.2)" = ">" ] && [ "$(thf_ver_cmp "$fzf_v" 0.23)" = ">" ]; then
        echo 1
    else
        echo 0
    fi
}

# Header line shown above the fzf list. Mentions the default action so the
# user knows what Enter will do, plus the multi-select hint.
thf_build_header() {
    local act
    act=$(printf '%s' "$THF_DEFAULT_ACTION" | tr '[:lower:]' '[:upper:]')
    printf 'TAB multi-select | Enter=%s | ESC=cancel | (scope=%s)' \
        "$act" "$THF_SCOPE"
}

# Invoke fzf, routing through fzf-tmux (popup inside tmux) when available so the
# picker renders as a centered popup instead of taking over the pane. All
# options are passed through as separate argv elements (no eval), which keeps
# quoting sane. Reads candidates from stdin, writes selection to stdout.
thf_fzf_invoke() {
    if thf_have fzf-tmux && [ -n "${TMUX:-}" ]; then
        # fzf-tmux accepts the same options as fzf plus -p/-w/-h for sizing.
        # Default to a large popup when the caller hasn't sized it.
        if [ "$(thf_use_popup)" = 1 ]; then
            fzf-tmux -p 80%,60% "$@"
        else
            fzf-tmux "$@"
        fi
    else
        # Plain fzf (e.g. when run from a terminal, not inside tmux).
        fzf "$@"
    fi
}

# Escape a string so it matches literally when used as a tmux copy-mode
# search pattern. tmux's search-forward/search-backward interpret the argument
# as a regular expression, so regex metacharacters must be backslash-escaped to
# match literally. A single character-class pass prepends a backslash to each
# metacharacter (the backslash itself is included in the class).
thf_regex_escape() {
    printf '%s' "$1" | sed 's/[][(){}.*+?^$|\\]/\\&/g'
}

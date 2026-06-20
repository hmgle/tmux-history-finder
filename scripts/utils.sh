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

# tmux socket. Empty when running outside of tmux (CLI mode against $TMUX or
# a passed socket); otherwise we talk to the same server the client uses.
thf_tmux_socket_args() {
    if [ -n "${THF_TMUX_ARGS:-}" ]; then
        printf '%s\n' "$THF_TMUX_ARGS"
    elif [ -n "${TMUX:-}" ]; then
        # Inside tmux: the default `tmux` invocation targets the right server.
        :
    fi
}

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
# Which backend to prefer for clipboard: pbcopy | wl-copy | xclip | xsel | clip.exe
# (auto-detected; this override is only needed for unusual setups).

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

# Build the case-sensitivity flags for the chosen backend.
thf_case_flags() {
    case "${1:-$THF_CASE}" in
        sensitive)   printf '%s' "-- " ;;  # both rg/grep default to sensitive
        insensitive) printf '%s' "-i" ;;
        smart|*)
            # rg has native smart-case (-S); grep emulates with -i only when the
            # pattern contains an uppercase letter (handled by caller).
            if [ "${2:-$(thf_resolve_backend)}" = rg ]; then
                printf '%s' "-S"
            else
                printf '%s' "--smart-case-placeholder"
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

# Pick the fzf launcher: system fzf-tmux if present, otherwise plain fzf.
thf_fzf_bin() {
    if thf_have fzf-tmux; then echo "fzf-tmux"
    else echo "fzf"; fi
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
# as a regular expression, so metacharacters like [ ] . * ( ) must be escaped
# to match the literal character. Backslash is escaped first (order matters).
thf_regex_escape() {
    printf '%s' "$1" | sed 's/[][(){}.*+?^$|\\]/\\&/g'
}

# Escape a string for safe use as a tmux argument (single-quoted).
thf_tmux_quote() {
    local s=$1
    s=${s//\'/\'\'\'}
    printf "'%s'" "$s"
}

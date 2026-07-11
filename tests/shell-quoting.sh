#!/usr/bin/env bash

set -euo pipefail
unset TMUX TMUX_PANE

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TMP="$(mktemp -d "${TMPDIR:-/tmp}/thf-shell-test.XXXXXX")"
SOCKET="thf-shell-test-$$"
REAL_TMUX="$(command -v tmux)"
cleanup() {
    "$REAL_TMUX" -L "$SOCKET" kill-server >/dev/null 2>&1 || true
    rm -rf "$TMP"
}
trap cleanup EXIT

plugin="$TMP/plugin space'quote"
mkdir -p "$plugin/scripts" "$TMP/bin"
cp "$ROOT/tmux_history_finder.tmux" "$plugin/tmux_history_finder.tmux"
cp "$ROOT/history_finder.sh" "$plugin/history_finder.sh"
cp "$ROOT/scripts/"*.sh "$plugin/scripts/"

cat > "$TMP/bin/tmux" <<'EOF'
#!/usr/bin/env bash
exec "$THF_TEST_REAL_TMUX" -L "$THF_TEST_SOCKET" "$@"
EOF
chmod +x "$TMP/bin/tmux"
export THF_TEST_REAL_TMUX="$REAL_TMUX"
export THF_TEST_SOCKET="$SOCKET"
export PATH="$TMP/bin:$PATH"

"$REAL_TMUX" -L "$SOCKET" -f /dev/null new-session -d -s quote 'sleep 60'
"$REAL_TMUX" -L "$SOCKET" set-option -g @tmux_history_finder_pane_key /
bash "$plugin/tmux_history_finder.tmux"

bindings="$("$REAL_TMUX" -L "$SOCKET" list-keys -T prefix)"
search_binding="$(printf '%s\n' "$bindings" | grep -E 'prefix[[:space:]]+g[[:space:]].*history_finder.sh search')"
pane_binding="$(printf '%s\n' "$bindings" | grep -E 'prefix[[:space:]]+/[[:space:]].*history_finder.sh search --scope pane')"
motion_binding="$(printf '%s\n' "$bindings" | grep -E 'prefix[[:space:]]+s[[:space:]].*motion-s.sh')"
printf '%s\n' "$search_binding" | grep -Fq 'plugin\\ space'
printf '%s\n' "$search_binding" | grep -Fq 'quote/history_finder.sh search'
printf '%s\n' "$pane_binding" | grep -Fq 'plugin\\ space'
printf '%s\n' "$pane_binding" | grep -Fq 'quote/history_finder.sh search --scope pane'
printf '%s\n' "$motion_binding" | grep -Fq 'plugin\\ space'
printf '%s\n' "$motion_binding" | grep -Fq 'quote/scripts/motion-s.sh'

command_path="$TMP/command space'quote"
cat > "$command_path" <<'EOF'
#!/bin/sh
printf 'quoted command ran\n'
EOF
chmod +x "$command_path"
# shellcheck source=../scripts/utils.sh
source "$ROOT/scripts/utils.sh"
quoted="$(thf_shell_quote "$command_path")"
[ "$(sh -c "$quoted")" = "quoted command ran" ]

echo "shell quoting tests passed"

#!/usr/bin/env bash

set -euo pipefail
unset TMUX TMUX_PANE

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN="${THF_BIN:-$ROOT/target/debug/thf}"
[ -x "$BIN" ] || { echo "manager integration: build thf first" >&2; exit 1; }

SOCKET="thf-manager-$$-$RANDOM"
TMP="$(mktemp -d "${TMPDIR:-/tmp}/thf-manager.XXXXXX")"
cleanup() {
    tmux -L "$SOCKET" kill-server >/dev/null 2>&1 || true
    rm -rf "$TMP"
}
trap cleanup EXIT

mkdir -p "$TMP/bin"
cat > "$TMP/bin/fzf" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
input="$(cat)"
case " $* " in
    *" --print-query "*)
        printf '%s\n' "${THF_TEST_QUERY:-test-name}"
        printf '%s\n' "$input" | sed -n '1p'
        ;;
    *)
        if [ -n "${THF_TEST_MATCH:-}" ]; then
            printf '%s\n' "$input" | grep -F -m1 "$THF_TEST_MATCH"
        else
            printf '%s\n' "$input" | sed -n "${THF_TEST_PICK:-1}p"
        fi
        ;;
esac
EOF
chmod +x "$TMP/bin/fzf"

pane_one="$(tmux -L "$SOCKET" -f /dev/null new-session -d -s manager -P -F '#{pane_id}' \
    "bash -lc 'printf ready-one; sleep 60'")"
pane_two="$(tmux -L "$SOCKET" split-window -d -t "$pane_one" -P -F '#{pane_id}' \
    "bash -lc 'printf ready-two; sleep 60'")"

run_manage() {
    PATH="$TMP/bin:$PATH" \
        THF_TMUX_ARGS="-L $SOCKET" \
        THF_MANAGER_FZF_OPTIONS="" \
        THF_MANAGER_CONFIRM=0 \
        "$BIN" manage "$@"
}

tmux -L "$SOCKET" select-pane -t "$pane_one"
run_manage pane switch
[ "$(tmux -L "$SOCKET" display-message -p '#{pane_id}')" = "$pane_two" ]

tmux -L "$SOCKET" copy-mode -t "$pane_two"
run_manage copy-mode cancel
[ "$(tmux -L "$SOCKET" display-message -p -t "$pane_two" '#{pane_in_mode}')" = 0 ]

tmux -L "$SOCKET" set-buffer -b manager-test 'manager-clipboard'
THF_TEST_MATCH="manager-clipboard" run_manage clipboard buffer
sleep 0.1
tmux -L "$SOCKET" capture-pane -p -t "$pane_two" | grep -Fq manager-clipboard

tmux -L "$SOCKET" bind-key -T prefix C-g set-option -g @manager_binding_ran yes
THF_TEST_MATCH="@manager_binding_ran" run_manage keybinding
[ "$(tmux -L "$SOCKET" show-option -gqv @manager_binding_ran)" = yes ]

THF_TEST_QUERY="created-manager-session" run_manage session new
tmux -L "$SOCKET" has-session -t created-manager-session
THF_TEST_MATCH="created-manager-session" run_manage session kill
if tmux -L "$SOCKET" has-session -t created-manager-session 2>/dev/null; then
    echo "manager integration: session kill left the selected session alive" >&2
    exit 1
fi

tmux -L "$SOCKET" new-window -d -t manager -n old-manager-window 'sleep 60'
THF_TEST_MATCH="old-manager-window" THF_TEST_QUERY="renamed-manager-window" \
    run_manage window rename
[ "$(tmux -L "$SOCKET" list-windows -t manager -F '#{window_name}' | rg -c '^renamed-manager-window$')" = 1 ]
THF_TEST_MATCH="renamed-manager-window" run_manage window kill
if tmux -L "$SOCKET" list-windows -t manager -F '#{window_name}' | \
    rg -q '^renamed-manager-window$'; then
    echo "manager integration: window kill left the selected window alive" >&2
    exit 1
fi

marker="$TMP/menu-ran"
THF_MANAGER_MENU="mark\nprintf '%s' done > '$marker'\n" run_manage menu
for _ in $(seq 1 50); do
    [ -f "$marker" ] && break
    sleep 0.02
done
[ "$(cat "$marker")" = "done" ]

sleep 60 &
child_pid=$!
THF_TEST_MATCH=" $child_pid " run_manage process terminate
for _ in $(seq 1 50); do
    ! kill -0 "$child_pid" 2>/dev/null && break
    sleep 0.02
done
if kill -0 "$child_pid" 2>/dev/null; then
    echo "manager integration: process signal did not terminate $child_pid" >&2
    exit 1
fi

echo "manager integration tests passed"

#!/usr/bin/env bash

set -euo pipefail
unset TMUX TMUX_PANE

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN="${TNX_BIN:-$ROOT/target/debug/tnx}"
[ -x "$BIN" ] || { echo "manager integration: build tnx first" >&2; exit 1; }

SOCKET="tnx-manager-$$-$RANDOM"
TMP="$(mktemp -d "${TMPDIR:-/tmp}/tnx-manager.XXXXXX")"
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
        printf '%s\n' "${TNX_TEST_QUERY:-test-name}"
        printf '%s\n' "$input" | sed -n '1p'
        ;;
    *)
        if [ -n "${TNX_TEST_MATCH:-}" ]; then
            printf '%s\n' "$input" | grep -F -m1 "$TNX_TEST_MATCH"
        else
            printf '%s\n' "$input" | sed -n "${TNX_TEST_PICK:-1}p"
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
        TNX_TMUX_ARGS="-L $SOCKET" \
        TNX_MANAGER_FZF_OPTIONS="" \
        TNX_MANAGER_CONFIRM=0 \
        "$BIN" manage "$@"
}

tmux -L "$SOCKET" select-pane -t "$pane_one"
run_manage pane switch
[ "$(tmux -L "$SOCKET" display-message -p '#{pane_id}')" = "$pane_two" ]

tmux -L "$SOCKET" copy-mode -t "$pane_two"
run_manage copy-mode cancel
[ "$(tmux -L "$SOCKET" display-message -p -t "$pane_two" '#{pane_in_mode}')" = 0 ]

tmux -L "$SOCKET" set-buffer -b manager-test 'manager-clipboard'
TNX_TEST_MATCH="manager-clipboard" run_manage clipboard buffer
sleep 0.1
tmux -L "$SOCKET" capture-pane -p -t "$pane_two" | grep -Fq manager-clipboard

tmux -L "$SOCKET" bind-key -T prefix C-g set-option -g @manager_binding_ran yes
TNX_TEST_MATCH="@manager_binding_ran" run_manage keybinding
[ "$(tmux -L "$SOCKET" show-option -gqv @manager_binding_ran)" = yes ]

TNX_TEST_QUERY="created-manager-session" run_manage session new
tmux -L "$SOCKET" has-session -t created-manager-session
TNX_TEST_MATCH="created-manager-session" run_manage session kill
if tmux -L "$SOCKET" has-session -t created-manager-session 2>/dev/null; then
    echo "manager integration: session kill left the selected session alive" >&2
    exit 1
fi

tmux -L "$SOCKET" new-window -d -t manager -n old-manager-window 'sleep 60'
TNX_TEST_MATCH="old-manager-window" TNX_TEST_QUERY="renamed-manager-window" \
    run_manage window rename
[ "$(tmux -L "$SOCKET" list-windows -t manager -F '#{window_name}' | grep -c '^renamed-manager-window$')" = 1 ]
TNX_TEST_MATCH="renamed-manager-window" run_manage window kill
if tmux -L "$SOCKET" list-windows -t manager -F '#{window_name}' | \
    grep -q '^renamed-manager-window$'; then
    echo "manager integration: window kill left the selected window alive" >&2
    exit 1
fi

linked_window="$(tmux -L "$SOCKET" new-window -d -t manager \
    -n linked-manager-window -P -F '#{window_id}' 'sleep 60')"
tmux -L "$SOCKET" new-session -d -s linked-other 'sleep 60'
tmux -L "$SOCKET" link-window -s "$linked_window" -t linked-other:
linked_index="$(tmux -L "$SOCKET" display-message -p \
    -t "manager:$linked_window" '#{window_index}')"
TNX_TEST_MATCH="manager:$linked_index: linked-manager-window" run_manage window kill
if tmux -L "$SOCKET" list-windows -t manager -F '#{window_name}' | \
    grep -q '^linked-manager-window$'; then
    echo "manager integration: selected window link was not removed" >&2
    exit 1
fi
if ! tmux -L "$SOCKET" list-windows -t linked-other -F '#{window_name}' | \
    grep -q '^linked-manager-window$'; then
    echo "manager integration: window kill removed the wrong linked window" >&2
    exit 1
fi
tmux -L "$SOCKET" kill-session -t linked-other

marker="$TMP/menu-ran"
TNX_MANAGER_MENU="mark\nprintf '%s' done > '$marker'\n" run_manage menu
for _ in $(seq 1 50); do
    [ -f "$marker" ] && break
    sleep 0.02
done
[ "$(cat "$marker")" = "done" ]

sleep 60 &
child_pid=$!
TNX_TEST_MATCH=" $child_pid " run_manage process terminate
for _ in $(seq 1 50); do
    ! kill -0 "$child_pid" 2>/dev/null && break
    sleep 0.02
done
if kill -0 "$child_pid" 2>/dev/null; then
    echo "manager integration: process signal did not terminate $child_pid" >&2
    exit 1
fi

tmux -L "$SOCKET" set-option -g @tmux_nexus_manager_key ''
TNX_TMUX_ARGS="-L $SOCKET" "$BIN" doctor | grep -Fq 'manager: key= order='

echo "manager integration tests passed"

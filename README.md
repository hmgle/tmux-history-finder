# tmux-history-finder

[![ci](https://github.com/hmgle/tmux-history-finder/actions/workflows/shellcheck.yml/badge.svg)](https://github.com/hmgle/tmux-history-finder/actions/workflows/shellcheck.yml)

Search the visible text and scrollback history of every tmux pane from one fast
picker, then jump to, copy, send, or print the selected match.

This branch uses a Rust backend (`thf`) for capture, indexing, search, preview,
and actions. The tmux plugin entry point and legacy shell script paths remain as
small compatibility wrappers.

It also includes a tmux-easymotion style visible-pane motion mode: jump to any
matching character currently visible in the active tmux window using short
on-screen hints.

## Requirements

- `tmux` 3.2+ recommended.
- `fzf` for the interactive picker.
- `curl` or `wget` to download the prebuilt backend (only on first run). No Rust
  toolchain is required to use the plugin; install one only to build from source.
- Optional: `fzf-tmux` for popup rendering, `rg` for user workflows, and one
  clipboard helper (`pbcopy`, `wl-copy`, `xclip`, `xsel`, or `clip.exe`).
- Optional manager integrations: `copyq` for system clipboard history and
  `pstree` for process trees. Without CopyQ, the manager uses tmux buffers.

## Install

With TPM:

```tmux
set -g @plugin 'hmgle/tmux-history-finder'
```

The wrapper `history_finder.sh` resolves the `thf` backend in this order:

1. `$THF_BIN`, if set to an executable.
2. A locally built binary (`target/release/thf` or `target/debug/thf`).
3. A current, previously downloaded binary (`bin/thf`).
4. `cargo run`, when a Rust toolchain is present (source checkouts/development).
5. Otherwise, a prebuilt release binary is downloaded for your platform via
   `scripts/install-binary.sh` (checksum-verified) into `bin/thf`.

Local and downloaded binaries are used only when their version matches
`Cargo.toml` and they are newer than the Rust sources. A plain tagged TPM
install therefore needs no toolchain: the first launch fetches the matching
prebuilt binary and caches it. Untagged source checkouts are never paired with a
release binary automatically; build those with Cargo instead. To prefetch a
tagged release explicitly (or in a post-install hook):

```sh
bash ~/.tmux/plugins/tmux-history-finder/scripts/install-binary.sh
```

Set `THF_AUTO_DOWNLOAD=0` to disable the download fallback, or `THF_BIN=/path/to/thf`
to use your own binary. For development, build the backend once:

```sh
cargo build --release
```

Manual install:

```tmux
run-shell /path/to/tmux-history-finder/tmux_history_finder.tmux
```

## Update

When installed with TPM, `prefix + U` or `update_plugins` updates the Git
checkout. The wrapper ignores stale local or cached binaries on the next run,
then rebuilds with Cargo or downloads the binary for an exact release tag.

If you build from source, rebuild the backend after updating:

```sh
~/.tmux/plugins/tpm/bin/update_plugins tmux-history-finder
cargo build --release --manifest-path ~/.tmux/plugins/tmux-history-finder/Cargo.toml
```

If you use the prebuilt release binary, refresh the cached binary explicitly:

```sh
~/.tmux/plugins/tpm/bin/update_plugins tmux-history-finder
rm -f ~/.tmux/plugins/tmux-history-finder/target/release/thf \
      ~/.tmux/plugins/tmux-history-finder/target/debug/thf
bash ~/.tmux/plugins/tmux-history-finder/scripts/install-binary.sh --force
```

Reloading `~/.tmux.conf` is usually not required unless the tmux plugin entry
script or tmux options changed.

## Usage

Use `history_finder.sh` as the standalone CLI entry point. From the plugin or
source directory, run it as `bash ./history_finder.sh`; from elsewhere, pass the
full script path. The tmux binding calls the same wrapper internally, and the
wrapper then resolves and execs the `thf` backend.

```sh
bash ./history_finder.sh                       # interactive, all panes
bash ./history_finder.sh error                 # pre-filter to matching records
bash ./history_finder.sh --scope session error # current session only
bash ./history_finder.sh --scope pane          # current pane only
bash ./history_finder.sh --history-lines 5000  # limit scrollback captured per pane
bash ./history_finder.sh --action copy token   # copy selected text
bash ./history_finder.sh --print panic         # non-interactive print
bash ./history_finder.sh --regex 'error|panic' # regex search
bash ./history_finder.sh motion s a            # visible-pane 1-char jump
bash ./history_finder.sh motion s2 he          # visible-pane 2-char jump
bash ./history_finder.sh manage                # full tmux workspace manager
bash ./history_finder.sh manage pane switch    # direct pane switcher
bash ./history_finder.sh manage clipboard      # CopyQ/tmux buffer history
bash ./history_finder.sh doctor                # dependency/config diagnostics
```

Inside fzf:

| Key      | Action                                 |
| -------- | -------------------------------------- |
| `Enter`  | Run the configured default action      |
| `TAB`    | Multi-select results                   |
| `Ctrl-y` | Copy selected result text              |
| `Ctrl-s` | Send selected text to the current pane |
| `Ctrl-p` | Print selected text to stdout          |
| `ESC`    | Cancel                                 |

Motion mode:

| Key                      | Action                                                                                  |
| ------------------------ | --------------------------------------------------------------------------------------- |
| `Prefix+s`               | Prompt for one character, draw hints over visible panes, then jump to the selected hint |
| configured `motion2_key` | Prompt for two characters and jump to the selected matching pair                        |

The two-character binding is disabled by default. Set
`@tmux_history_finder_motion2_key` to enable it.

Workspace manager:

| Category | Actions |
| --- | --- |
| `session` | switch, new, rename, detach, kill |
| `window` | switch, link, move, swap, rename, unlink/kill |
| `pane` | switch, break, join, swap, layout, kill, resize |
| `copy-mode` | select and execute copy-mode commands |
| `command` | insert a tmux command into `command-prompt` |
| `keybinding` | inspect and execute a configured key binding |
| `clipboard` | preview and paste CopyQ history or tmux buffers |
| `process` | top, pstree, TERM, KILL, INT, CONT, STOP, QUIT, HUP |
| `menu` | execute user-defined commands, optionally in a popup |

`Prefix+F` opens the manager. Destructive actions require confirmation by
default. TAB selects multiple targets for detach, kill, join, clipboard paste,
and process signals. Object actions use hidden tmux IDs, so custom formats may
contain spaces, colons, quotes, and duplicate display text safely.

## Configuration

Set options in tmux:

```tmux
set -g @tmux_history_finder_launch_key "g"
set -g @tmux_history_finder_pane_key "/"
set -g @tmux_history_finder_default_action "jump"
set -g @tmux_history_finder_scope "all"
set -g @tmux_history_finder_prompt_query "0"
set -g @tmux_history_finder_history_lines "0"
set -g @tmux_history_finder_motion_key "s"
set -g @tmux_history_finder_motion2_key "S"
set -g @tmux_history_finder_motion_hints "asdghklqwertyuiopzxcvbnmfj;"
set -g @tmux_history_finder_manager_key "F"
set -g @tmux_history_finder_manager_fzf_options "-p -w 62% -h 38% -m"
set -g @tmux_history_finder_manager_confirm "1"
```

Or use environment variables:

```sh
THF_DEFAULT_ACTION=copy bash ./history_finder.sh token
THF_TMUX_ARGS='-L work' bash ./history_finder.sh --scope session error
```

Supported values:

| Option / env var                                                | Default                       | Values                                                  |
| --------------------------------------------------------------- | ----------------------------- | ------------------------------------------------------- |
| `launch_key` / `THF_LAUNCH_KEY`                                 | `g`                           | tmux prefix binding                                     |
| `pane_key`                                                      | empty                         | tmux prefix binding for current-pane search             |
| `scope` / `THF_SCOPE`                                           | `all`                         | `all`, `session`, `pane`                                |
| `include_history` / `THF_INCLUDE_HISTORY`                       | `1`                           | `1` or `0`                                              |
| `history_lines` / `THF_HISTORY_LINES`                           | `0`                           | `0` for all history, or a positive line count           |
| `case` / `THF_CASE`                                             | `smart`                       | `smart`, `sensitive`, `insensitive`                     |
| `join_wraps` / `THF_JOIN_WRAPS`                                 | `1`                           | `1` or `0`                                              |
| `skip_blank` / `THF_SKIP_BLANK`                                 | `1`                           | `1` or `0`                                              |
| `preview` / `THF_PREVIEW`                                       | `1`                           | `1` or `0`                                              |
| `prompt_query` / `THF_PROMPT_QUERY`                             | `0`                           | `1` asks for a query before capturing panes             |
| `default_action` / `THF_DEFAULT_ACTION`                         | `jump`                        | `jump`, `copy`, `send`, `print`                         |
| `fzf_options` / `THF_FZF_OPTIONS`                               | empty                         | extra fzf arguments                                     |
| `motion_key` / `THF_MOTION_KEY`                                 | `s`                           | tmux prefix binding for 1-character visible-pane motion |
| `motion2_key` / `THF_MOTION2_KEY`                               | empty                         | tmux prefix binding for 2-character visible-pane motion |
| `motion_hints` / `THF_MOTION_HINTS`                             | `asdghklqwertyuiopzxcvbnmfj;` | characters used for motion hints                        |
| `motion_case` / `THF_MOTION_CASE`                               | `insensitive`                 | `smart`, `sensitive`, `insensitive`                     |
| `motion_smartsign` / `THF_MOTION_SMARTSIGN`                     | `0`                           | `1` also matches shifted symbols such as `1` -> `!`     |
| `motion_copy_mode_no_prefix` / `THF_MOTION_COPY_MODE_NO_PREFIX` | `0`                           | bind motion keys directly in copy-mode tables           |
| `motion_vertical_border` / `THF_MOTION_VERTICAL_BORDER`         | `│`                           | vertical border used by the motion overlay               |
| `motion_horizontal_border` / `THF_MOTION_HORIZONTAL_BORDER`     | `─`                           | horizontal border used by the motion overlay             |
| `motion_hint1_fg` / `THF_MOTION_HINT1_FG`                       | `1;31`                        | SGR color for the first hint character                  |
| `motion_hint2_fg` / `THF_MOTION_HINT2_FG`                       | `1;32`                        | SGR color for the second hint character                 |
| `motion_dim` / `THF_MOTION_DIM`                                 | `2`                           | SGR color for dimmed pane borders                       |

Manager settings use `@tmux_history_finder_manager_*` tmux options or
`THF_MANAGER_*` environment variables:

| Setting | Default | Purpose |
| --- | --- | --- |
| `key` | `F` | tmux prefix binding; set it to an empty value to disable |
| `order` | `history|copy-mode|session|window|pane|command|keybinding|clipboard|process` | category order and visibility |
| `fzf_options` | `-p -w 62% -h 38%` | fzf-tmux popup/layout options |
| `preview`, `preview_follow` | `1`, `1` | preview visibility and follow mode |
| `confirm` | `1` | confirm destructive actions |
| `switch_current` | `0` | include the current object in switch lists |
| `session_format`, `window_format`, `pane_format` | tmux-aware defaults | custom list formats |
| `window_filter` | empty | tmux `list-windows -f` filter |
| `menu` | empty | legacy-compatible label/command pairs |
| `menu_popup` | `0` | run menu commands in a popup |
| `menu_popup_width`, `menu_popup_height` | `50%`, `50%` | menu popup size |

For migration, the manager also reads the corresponding `TMUX_FZF_*`
variables when no new setting is present. The precedence is `THF_MANAGER_*`,
tmux option, legacy `TMUX_FZF_*`, then the default.

Menu entries use the tmux-fzf-compatible format:

```tmux
set -g @tmux_history_finder_manager_menu "foo\necho Hello\n\nstatus\ntmux display-message ready\n"
```

Direct bindings do not need compatibility scripts:

```tmux
bind-key f run-shell -b "~/.tmux/plugins/tmux-history-finder/history_finder.sh manage pane switch"
bind-key y run-shell -b "~/.tmux/plugins/tmux-history-finder/history_finder.sh manage clipboard"
```

### Migrating from sainnhe/tmux-fzf

Remove `set -g @plugin 'sainnhe/tmux-fzf'`, keep the manager binding at its
default `Prefix+F`, and translate persistent configuration as follows:

```tmux
set -g @tmux_history_finder_manager_fzf_options "-p -w 86% -h 58% -m"
set -g @tmux_history_finder_manager_pane_format \
  "#{b:pane_current_path} #{=/-26/...:#{d:pane_current_path}} [#{pane_current_command}]"
```

Legacy environment variables remain supported, but the tmux options above are
preferred for new configurations.

CLI flags override configuration for that run.

`pane_key` is disabled by default so the plugin does not replace tmux's existing
binding. When enabled, the plugin resolves `history_finder.sh` relative to its
own installation directory, so the binding does not depend on TPM's path.

Motion hints use the configured characters as a prefix-free key set. When a
common match produces more targets than one- or two-character hints can cover,
farther targets use longer hints instead of being dropped.

When `prompt_query` is enabled for the tmux binding, pressing the launch key
opens a tmux prompt first. Empty input cancels without capturing pane history.
This is useful for large tmux servers where opening an unfiltered all-pane
picker would capture and index more scrollback than needed.

## How It Works

1. `capture` lists panes in scope and captures them in parallel, using
   `history_lines` when a scrollback limit is configured.
2. The Rust backend builds a structured temporary index: pane snapshots plus
   record IDs for searchable lines.
3. Search filters the in-memory index using literal matching by default, or
   regex matching with `--regex`.
4. fzf displays compact rows while preview and actions resolve the selected
   record ID against the same snapshot.
5. Actions call tmux directly to jump, copy, send, or print.

Motion mode uses a separate visible-screen path. It lists the panes in the
current tmux window once, starts their independent `capture-pane` commands
concurrently, and preserves pane order while collecting the results. It then
searches the visible text and draws an ANSI hint overlay in a borderless tmux
popup. Selecting a target sends the ordered window, pane, copy-mode, and cursor
operations to tmux as one command sequence instead of starting a tmux process
for every cursor step.

## Development

```sh
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
env -u TMUX -u TMUX_PANE cargo test --locked
cargo build --locked
shellcheck -x --source-path=SCRIPTDIR \
  history_finder.sh tmux_history_finder.tmux scripts/*.sh tests/*.sh
bash tests/install-binary.sh
env -u TMUX -u TMUX_PANE bash tests/shell-quoting.sh
env -u TMUX -u TMUX_PANE bash tests/tmux-integration.sh
env -u TMUX -u TMUX_PANE bash tests/manage-integration.sh
```

The tmux integration tests require `tmux`; picker workflows and CI also require
`fzf`. Every test server uses its own explicit tmux socket.

Use `bash ./history_finder.sh doctor` to verify local dependencies and resolved
configuration.

## Inspired by

- [tmux-fzf](https://github.com/sainnhe/tmux-fzf)
- [tmux-easymotion](https://github.com/ddzero2c/tmux-easymotion)

## License

MIT. See [LICENSE](LICENSE).

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

## Install

With TPM:

```tmux
set -g @plugin 'hmgle/tmux-history-finder'
```

The wrapper `history_finder.sh` resolves the `thf` backend in this order:

1. `$THF_BIN`, if set to an executable.
2. A locally built binary (`target/release/thf` or `target/debug/thf`).
3. A previously downloaded binary (`bin/thf`).
4. `cargo run`, when a Rust toolchain is present (source checkouts/development).
5. Otherwise, a prebuilt release binary is downloaded for your platform via
   `scripts/install-binary.sh` (checksum-verified) into `bin/thf`.

So a plain TPM install needs no toolchain: the first launch fetches the matching
prebuilt binary and caches it. To prefetch it explicitly (or in a post-install
hook):

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

When installed with TPM, `prefix + U` or `update_plugins` updates only the Git
checkout. It does not rebuild local Cargo artifacts or replace an already
cached backend binary.

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
bash ./history_finder.sh doctor                # dependency/config diagnostics
```

Inside fzf:

| Key      | Action                                      |
| -------- | ------------------------------------------- |
| `Enter`  | Run the configured default action           |
| `TAB`    | Multi-select results                        |
| `Ctrl-y` | Copy selected result text                   |
| `Ctrl-s` | Send selected text to the current pane      |
| `Ctrl-p` | Print selected text to stdout               |
| `ESC`    | Cancel                                      |

Motion mode:

| Key | Action |
| --- | --- |
| `Prefix+s` | Prompt for one character, draw hints over visible panes, then jump to the selected hint |
| configured `motion2_key` | Prompt for two characters and jump to the selected matching pair |

The two-character binding is disabled by default. Set
`@tmux_history_finder_motion2_key` to enable it.

## Configuration

Set options in tmux:

```tmux
set -g @tmux_history_finder_launch_key "g"
set -g @tmux_history_finder_default_action "jump"
set -g @tmux_history_finder_scope "all"
set -g @tmux_history_finder_prompt_query "0"
set -g @tmux_history_finder_history_lines "0"
set -g @tmux_history_finder_motion_key "s"
set -g @tmux_history_finder_motion2_key "S"
set -g @tmux_history_finder_motion_hints "asdghklqwertyuiopzxcvbnmfj;"
```

Or use environment variables:

```sh
THF_DEFAULT_ACTION=copy bash ./history_finder.sh token
THF_TMUX_ARGS='-L work' bash ./history_finder.sh --scope session error
```

Supported values:

| Option / env var | Default | Values |
| --- | --- | --- |
| `launch_key` / `THF_LAUNCH_KEY` | `g` | tmux prefix binding |
| `scope` / `THF_SCOPE` | `all` | `all`, `session`, `pane` |
| `include_history` / `THF_INCLUDE_HISTORY` | `1` | `1` or `0` |
| `history_lines` / `THF_HISTORY_LINES` | `0` | `0` for all history, or a positive line count |
| `case` / `THF_CASE` | `smart` | `smart`, `sensitive`, `insensitive` |
| `join_wraps` / `THF_JOIN_WRAPS` | `1` | `1` or `0` |
| `skip_blank` / `THF_SKIP_BLANK` | `1` | `1` or `0` |
| `preview` / `THF_PREVIEW` | `1` | `1` or `0` |
| `prompt_query` / `THF_PROMPT_QUERY` | `0` | `1` asks for a query before capturing panes |
| `default_action` / `THF_DEFAULT_ACTION` | `jump` | `jump`, `copy`, `send`, `print` |
| `fzf_options` / `THF_FZF_OPTIONS` | empty | extra fzf arguments |
| `motion_key` / `THF_MOTION_KEY` | `s` | tmux prefix binding for 1-character visible-pane motion |
| `motion2_key` / `THF_MOTION2_KEY` | empty | tmux prefix binding for 2-character visible-pane motion |
| `motion_hints` / `THF_MOTION_HINTS` | `asdghklqwertyuiopzxcvbnmfj;` | characters used for motion hints |
| `motion_case` / `THF_MOTION_CASE` | `insensitive` | `smart`, `sensitive`, `insensitive` |
| `motion_smartsign` / `THF_MOTION_SMARTSIGN` | `0` | `1` also matches shifted symbols such as `1` -> `!` |
| `motion_copy_mode_no_prefix` / `THF_MOTION_COPY_MODE_NO_PREFIX` | `0` | bind motion keys directly in copy-mode tables |
| `motion_hint1_fg` / `THF_MOTION_HINT1_FG` | `1;31` | SGR color for the first hint character |
| `motion_hint2_fg` / `THF_MOTION_HINT2_FG` | `1;32` | SGR color for the second hint character |
| `motion_dim` / `THF_MOTION_DIM` | `2` | SGR color for dimmed pane borders |

CLI flags override configuration for that run.

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

Motion mode uses a separate visible-screen path. It captures only the panes in
the current tmux window, searches their visible text, draws an ANSI hint overlay
in a temporary tmux window, then selects the target pane and moves the copy-mode
cursor to the selected screen row and column.

## Development

```sh
cargo fmt --check
cargo test
cargo build
shellcheck -x --source-path=SCRIPTDIR history_finder.sh tmux_history_finder.tmux scripts/*.sh
```

Use `bash ./history_finder.sh doctor` to verify local dependencies and resolved
configuration.

## License

MIT. See [LICENSE](LICENSE).

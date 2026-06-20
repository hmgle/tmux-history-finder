# tmux-history-finder

Search the **visible text and scrollback history of every tmux pane** from a
single fuzzy finder, then jump to, copy, send, or print the match.

Inspired by [tmux-fzf](https://github.com/sainnhe/tmux-fzf), but focused on one
job: find matches across _all_ panes (across all sessions) and act on the result.

- **Scope**: search every pane in every session, or limit to the current
  session / current pane.
- **History**: includes full scrollback by default (not just the visible screen).
- **Fast**: pre-filters the index with `ripgrep` (or `grep`) before handing the
  narrowed list to `fzf`, so even huge histories stay snappy.
- **Preview**: each candidate shows a window of surrounding lines from its
  source pane, with the match highlighted.
- **Actions**: `jump` (switch to the pane and land the cursor on the match via
  copy-mode search), `copy` (tmux buffer + system clipboard), `send` (type the
  text into the current pane), or `print` (to stdout, for scripting).

## Requirements

- `tmux` 3.2+ (for popup / `display-popup`); older versions work but render
  inline instead of as a popup.
- `fzf` (0.23+ recommended for popup support via `fzf-tmux`).
- `ripgrep` (`rg`) — optional but recommended; falls back to `grep`.
- `bash` 3.2+ (works with the macOS system bash; no bash-4-only features are
  used).

## Install

### With [TPM](https://github.com/tmux-plugins/tpm)

Add to `~/.tmux.conf`:

```tmux
set -g @plugin 'hmgle/tmux-history-finder'
```

Then press `prefix` + <kbd>I</kbd> to install. The default binding
`prefix` + <kbd>g</kbd> opens the search.

### Manual

Clone anywhere, then source the entry point from `tmux.conf`:

```tmux
run-shell /path/to/tmux-history-finder/tmux_history_finder.tmux
```

### As a standalone CLI

Symlink (or add to `PATH`) the wrapper script:

```sh
ln -s /path/to/tmux-history-finder/history_finder.sh ~/.local/bin/history-finder
```

## Usage

### Interactive (inside tmux)

```
prefix + g        # open the picker across all panes
```

Type to narrow results. Key bindings inside the picker:

| Key     | Action                                            |
| ------- | ------------------------------------------------- |
| `Enter` | Run the default action (default: `jump`)          |
| `TAB`   | Multi-select (then `Enter` acts on each selected) |
| `ESC`   | Cancel                                            |
| preview | Auto-shown on the right; the match is highlighted |

### From the command line

```sh
history-finder                       # interactive, all panes
history-finder 'error'               # pre-filtered to lines matching 'error'
history-finder --scope session foo   # current session only
history-finder --scope pane          # current pane only
history-finder --action copy 'token' # copy the selected line to clipboard
history-finder --print 'panic'       # print matching lines (scriptable, no UI)
history-finder --no-history          # visible screen only (ignore scrollback)
history-finder --no-join             # don't join wrapped lines
history-finder --case sensitive Foo  # force case sensitivity
history-finder --version             # print the version and exit
```

### Actions

- **`jump`** (default) — switch the client to the result's pane, enter
  `copy-mode`, and run `search-forward` so the cursor lands on the matched
  text. Locating by text (rather than a line number) is robust to blank-line
  skipping and line-wrap joining, which would otherwise shift the coordinate.
- **`copy`** — put the matched line into the tmux paste buffer and the system
  clipboard (`pbcopy` / `wl-copy` / `xclip` / `xsel`, auto-detected).
- **`send`** — type the matched text into the _current_ pane's active program.
- **`print`** — write the matched line to stdout (useful for piping into other
  tools).

## Configuration

All options can be set two ways:

1. **tmux `@`-options** (recommended inside `tmux.conf`):
   ```tmux
   set -g @tmux_history_finder_launch_key "g"
   set -g @tmux_history_finder_default_action "copy"
   set -g @tmux_history_finder_scope "session"
   ```
2. **Environment variables** (handy for the CLI):
   ```sh
   THF_DEFAULT_ACTION=copy history-finder 'foo'
   ```

| Option (`@tmux_history_finder_*` / `THF_*`) | Default   | Values / meaning                                        |
| ------------------------------------------- | --------- | ------------------------------------------------------- |
| `launch_key`                                | `g`       | Key (under `prefix`) that opens the picker.             |
| `scope`                                     | `all`     | `all` \| `session` \| `pane` — which panes to search.   |
| `include_history`                           | `1`       | `1` include scrollback, `0` visible screen only.        |
| `case`                                      | `smart`   | `smart` \| `sensitive` \| `insensitive`.                |
| `backend`                                   | `auto`    | `auto` \| `rg` \| `grep` — pre-filter engine.           |
| `join_wraps`                                | `1`       | `1` join visually-wrapped lines into one, `0` keep raw. |
| `skip_blank`                                | `1`       | `1` drop empty/whitespace-only lines from the index.    |
| `preview`                                   | `1`       | `1` show the source-pane preview in fzf.                |
| `default_action`                            | `jump`    | `jump` \| `copy` \| `send` \| `print`.                  |
| `fzf_options`                               | _(empty)_ | Extra options appended to the fzf invocation.           |

## How it works

1. **`capture.sh`** iterates every pane in scope, runs `tmux capture-pane` on
   each (with scrollback), and emits a TAB-separated index:
   `pane_id  location  command  window_name  line_no  text`.
2. **`search.sh`** pre-filters that index with `rg`/`grep`, hands the narrowed
   list to `fzf` (via `fzf-tmux` for the popup), and dispatches each selected
   record to the action handler.
3. **`preview.sh`** re-captures the candidate's source pane and shows a window
   of surrounding lines, locating the match by its text.
4. **`action.sh`** performs the chosen action. For `jump`, it uses copy-mode's
   `search-forward` with a regex-escaped literal of the matched text, so the
   cursor lands exactly on the match regardless of coordinate drift.

## License

MIT. See [LICENSE](LICENSE).

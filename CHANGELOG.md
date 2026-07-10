# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.5.0] - 2026-07-10

### Added
- Visible-pane motion mode can now jump to matching text in the current tmux
  window with easymotion-style hints. `Prefix+s` starts one-character motion,
  and `@tmux_history_finder_motion2_key` can enable two-character motion.
- Isolated tmux integration coverage now exercises visible-only duplicate
  jumps, limited history, wrapped lines, disappearing panes, and cross-pane
  motion navigation.

### Changed
- Motion overlays now render in a borderless tmux popup instead of a temporary
  tmux window.
- Search records now store compact pane and line coordinates instead of cloned
  text and metadata. Preview snapshots are split per pane and legacy capture is
  streamed, substantially reducing peak memory on large histories.
- Motion popup startup reuses its initial visible-pane snapshot, and overlay
  rendering batches terminal writes instead of repeating capture and flush
  work.
- Cached backends are accepted only when their version matches the source and
  they are newer than Rust inputs. Untagged source checkouts no longer download
  an unrelated release binary automatically.
- Motion internals are split into capture, matching, hint, rendering, terminal,
  and navigation modules.

### Fixed
- Motion hints can grow beyond two characters, so common matches no longer
  silently drop distant targets.
- Motion overlays restore tab cells as expanded screen spaces after hint
  filtering.
- Visible-only and limited-history jumps now preserve the absolute scrollback
  offset, including when the same text also appears earlier in history.
- Literal case-insensitive search now handles Unicode, while fzf cancellation,
  invocation errors, and prefilter queries retain their distinct semantics.
- Invalid configuration, malformed shell options, pane capture failures, jump
  failures, and tmux-buffer failures now return actionable errors instead of
  silently falling back or dropping data.
- Deferred tmux commands now quote paths and values safely, and fzf display
  fields are sanitized before rendering.
- Motion raw mode now restores terminal state on Ctrl-C and error paths, drains
  trailing escape bytes, and avoids drawing or restoring cells across pane and
  wide-character boundaries.

### Security
- Prebuilt installation now requires a valid checksum, rejects unsafe archive
  paths, serializes concurrent installs, and replaces the cached binary
  atomically only after every download and verification step succeeds.

## [0.4.1] - 2026-06-22

### Changed
- Loading configuration now reads all `@tmux_history_finder_*` tmux options in
  one call and reuses the result, avoiding repeated `show-option` calls after
  config import.
- Legacy capture output is now streamed directly from captured pane text instead
  of building a full structured search index first.
- Literal search now checks record fields directly and avoids per-record
  searchable-text and lowercase string allocations for common queries.

### Fixed
- Literal and regex searches continue to match across location, command, window,
  and text fields when queries include explicit tab separators.
- Legacy TSV capture preserves raw capture-pane line numbers when blank lines
  are skipped, so limited-history output reports the original tmux line numbers.

## [0.4.0] - 2026-06-22

### Added
- `history_lines` / `THF_HISTORY_LINES` and `--history-lines` can now limit how
  much scrollback is captured from each pane.
- `prompt_query` / `THF_PROMPT_QUERY` can make the tmux binding ask for a query
  before capturing pane history; empty input cancels without indexing panes.

### Changed
- Record lookup now uses direct record IDs, avoiding quadratic candidate
  generation when opening the picker without an initial query.

### Fixed
- Documented `bash ./history_finder.sh` as the standalone CLI entry point
  instead of the non-existent `history-finder` command.
- `history_lines` now preserves the omitted scrollback offset so jump actions
  still target the selected line after a limited capture.
- Prompted tmux searches now store the query under a client-specific temporary
  option, avoiding accidental reuse across concurrent prompts.

## [0.3.1] - 2026-06-22

### Fixed
- `Ctrl-y` and other fzf expect-key exits no longer fail with `BrokenPipe` when
  fzf closes before all candidates have been written.
- Copy now tries every available system clipboard helper before falling back to
  the tmux buffer, so an unusable helper such as `wl-copy` no longer blocks a
  working fallback such as `xclip`.
- The tmux-buffer fallback message now distinguishes between missing clipboard
  helpers and helpers that were present but failed.
- The release workflow now builds `x86_64-apple-darwin` on GitHub's supported
  `macos-15-intel` runner instead of the retired `macos-13` image.
- Release notes are now extracted from this changelog and published once per
  release, avoiding duplicated generated `Full Changelog` lines.

## [0.3.0] - 2026-06-21

### Changed
- Reworked the implementation around a Rust `thf` backend. The tmux plugin file
  and legacy shell paths remain as compatibility wrappers, but capture, search,
  preview, and actions now run through structured Rust code.
- Capture now runs panes in parallel and stores one structured pane snapshot per
  search. fzf preview and result actions resolve selected record IDs against the
  snapshot instead of repeatedly re-capturing full scrollback.
- Search now supports built-in literal matching by default and `--regex` for
  regular expressions, with smart/sensitive/insensitive case handling.

### Added
- `thf doctor` / `bash ./history_finder.sh doctor` diagnostics for tmux, fzf,
  fzf-tmux, ripgrep, clipboard support, and resolved configuration.
- fzf action shortcuts: `Ctrl-y` copy, `Ctrl-s` send, and `Ctrl-p` print, while
  `Enter` continues to use the configured default action.
- Rust CI coverage for formatting, unit tests, and build verification.
- Prebuilt `thf` binaries published to GitHub Releases (Linux/macOS, x86_64 and
  aarch64) via a tag-triggered `release` workflow.
- `scripts/install-binary.sh` downloads and checksum-verifies the prebuilt binary
  for the current platform into `bin/thf`. `history_finder.sh` calls it
  automatically when no Rust toolchain is available, so TPM installs work without
  `cargo`. Opt out with `THF_AUTO_DOWNLOAD=0`, or point `THF_BIN` at your own
  binary.

## [0.2.0] - 2026-06-20

### Fixed
- `@tmux_history_finder_*` options are now honoured by the key binding (and the
  CLI). They were previously exported from the transient plugin-load shell and
  lost before the picker ran, so only `launch_key` ever took effect. All options
  are now resolved at run time in `scripts/utils.sh`.
- `--case sensitive` with the ripgrep backend no longer mis-parses its flags. It
  used to emit a stray `-- ` that made `rg` treat the query as a filename and
  match the literal `--` instead, also corrupting the TAB record format.
- The `preview` option (`@tmux_history_finder_preview` / `THF_PREVIEW`) is now
  respected; the preview was previously always shown.
- `--print` with a query now runs fully non-interactively (no picker), matching
  its documented "scriptable, no UI" behaviour; it previously still launched fzf.
- The first-load notice now actually fires once. Its guard tested the exit
  status of `show-option -gqv`, which always succeeds, so the message (and the
  `@thf_loaded` marker) never appeared; it now tests the option's value.

### Changed
- The fzf preview caches each pane's capture for the picker's lifetime instead
  of re-capturing the full scrollback on every keystroke, and matches the
  index's wrap-join setting so the displayed line numbers line up.
- Relaxed the documented requirement from bash 4+ to bash 3.2+ (no bash-4-only
  features are used; works with the macOS system bash).

### Added
- `bash ./history_finder.sh --version` / `bash ./history_finder.sh -V`.

### Removed
- Unused internal helpers (`thf_tmux_socket_args`, `thf_fzf_bin`,
  `thf_tmux_quote`) and a dead smart-case placeholder.

## [0.1.0] - 2026-06-20

### Added
- Initial release.
- Search the visible content and full scrollback history of every tmux pane
  (across all sessions), with `all` / `session` / `pane` scope control.
- Interactive fuzzy finder (fzf) with a live source-pane preview that highlights
  the matched line.
- Four result actions: `jump` (switch to pane + copy-mode search to the match),
  `copy` (tmux buffer + system clipboard), `send` (type into current pane), and
  `print` (to stdout).
- Pre-filtering backend selection (`auto` / `rg` / `grep`) with smart-case
  support, so large histories stay responsive.
- TPM plugin entry point with a configurable launch key, plus a standalone
  `history_finder.sh` CLI wrapper.
- Full configuration via either tmux `@tmux_history_finder_*` options or `THF_*`
  environment variables.

### Notes
- The `jump` action locates the match by text via copy-mode `search-forward`
  (with regex metacharacters escaped) rather than by line number, so it is
  robust to blank-line skipping and line-wrap joining in the index.

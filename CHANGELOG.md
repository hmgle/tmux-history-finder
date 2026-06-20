# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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

### Changed
- The fzf preview caches each pane's capture for the picker's lifetime instead
  of re-capturing the full scrollback on every keystroke, and matches the
  index's wrap-join setting so the displayed line numbers line up.
- Relaxed the documented requirement from bash 4+ to bash 3.2+ (no bash-4-only
  features are used; works with the macOS system bash).

### Added
- `history-finder --version` / `-V`.

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

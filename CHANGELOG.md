# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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

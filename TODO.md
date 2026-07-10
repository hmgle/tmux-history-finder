# Review Remediation TODO

This checklist tracks the implementation of the repository-wide review.
Items are ordered by user impact and intended commit sequence.

## 1. Critical correctness

- [ ] Bump the development version so unreleased source never downloads the
  incompatible `v0.4.1` backend.
- [ ] Refuse automatic release downloads from an untagged Git checkout and
  verify an existing cached backend version before using it.
- [x] Make motion hint generation terminate for zero or one distinct hint key.
- [x] Replace the quadratic hint expansion algorithm with bounded linear work.
- [x] Preserve the full-history line offset for visible-only captures.
- [ ] Add a regression test for jumping to a visible duplicate with
  `--no-history`.
- [x] Stop passing Rust prefilter queries through fzf's unrelated query syntax.
- [x] Distinguish fzf cancellation from invocation and configuration failures.
- [x] Make case-insensitive literal search Unicode-aware.

## 2. Configuration and error handling

- [x] Return actionable errors for invalid environment and tmux option values.
- [x] Reject conflicting CLI flags such as `--history --no-history`.
- [x] Preserve pane capture failures instead of silently dropping panes.
- [x] Propagate jump and tmux-buffer failures with operation context.
- [x] Replace malformed shell-word fallback parsing with explicit errors.
- [ ] Introduce a typed, injectable tmux client instead of process-global test
  configuration.

## 3. Memory and startup performance

- [ ] Store record coordinates instead of cloning pane text and metadata per
  searchable line.
- [ ] Remove unused `before`, `after`, and logical line fields.
- [ ] Stream index serialization instead of building a second JSON buffer.
- [ ] Store pane snapshots separately so fzf preview loads only one pane.
- [ ] Stream legacy TSV output to the destination writer with bounded memory.
- [ ] Avoid rebuilding action targets and clipboard discovery per selected row.
- [ ] Avoid duplicate capture/search work when motion opens its popup.
- [ ] Sort pane references rather than cloning pane contents.
- [ ] Batch ANSI screen writes once per refresh.

## 4. Installer, shell, and terminal safety

- [ ] Make binary installation fail closed on command, checksum, or permission
  errors.
- [ ] Serialize concurrent first-run installations and atomically replace the
  cached binary.
- [ ] Validate archive contents before extraction.
- [ ] Quote all deferred `tmux run-shell` commands for paths containing spaces
  or quotes.
- [x] Sanitize fzf display fields and remove unnecessary ANSI interpretation.
- [x] Validate motion hints, border cells, and SGR configuration.
- [ ] Use a real raw terminal mode that allows graceful Ctrl-C cleanup.
- [ ] Drain trailing escape-sequence bytes on overlay cancellation.
- [ ] Prevent hint restoration and drawing from crossing pane boundaries.

## 5. Architecture and tests

- [ ] Split `motion.rs` into capture, matching, hint, rendering, terminal, and
  navigation modules.
- [x] Add unit tests for hint termination/prefix freedom, Unicode search,
  configuration errors, fzf statuses, and display sanitization.
- [ ] Add isolated tmux integration tests for visible-only duplicate jumps,
  limited history, wrapped lines, disappearing panes, and cross-pane motion.
- [ ] Make tmux integration prerequisites explicit instead of silently passing.
- [ ] Add installer tests for checksum absence, permission failure, and
  concurrent invocation.
- [ ] Install tmux and fzf explicitly in CI and run all integration tests.
- [ ] Run formatting, clippy, unit/integration tests, shellcheck, and a release
  build before marking this checklist complete.

## Contract Baseline

- `cargo test --locked --lib release_ -- --test-threads=1`: 10 tests passed,
  including wrong-claim rejection, busy admission, and dirty-worktree retention.
- `cargo test --locked --test status --test workspace_open --test codex_cli`:
  12 tests passed, including nested/removed boundaries, missing storage,
  invalid directories, ownership checks, and forwarded arguments.
- `openspec validate add-workspace-locator --strict --no-interactive`: passed.

Existing regression tests establish the compatibility baseline. New locator
and CLI tests will cover added selection forms and direct-construction conflicts.

## Shared Lookup

- `cargo test --locked --lib workspace_locator`: four tests passed.
- Coverage includes exact and ancestor boundaries, removed records, claim
  identity, absent targets, source errors, symlinks, and missing final components.
- Relative CLI path conversion will receive additional coverage in CLI integration.

## Consumer Migration

- `cargo test --locked --lib`: 212 tests passed.
- Selected binary and integration suites: 44 tests passed across command helpers,
  status, open, managed-session preparation, forwarding, and workspace reuse.
- Removed the release and status selector enums and both old ancestor algorithms.
  Transaction admission and removal dispatch retain their existing storage reads.

## CLI Integration

- `cargo test --locked --lib cli::`: 30 tests passed. The shared parser matrix
  covers all pairwise conflicts, repeated named selectors, malformed IDs,
  defaults, help, and conversion through each command.
- `cargo test --locked --test workspace_locator_cli`: two integration tests
  passed, exercising all named selectors, status views/headings, program launch,
  release cycles, missing claims, and automatic-workspace eligibility.
- Direct conversion tests reject conflicts before path access and verify that
  explicit identifiers do not resolve the current directory.

## Final Verification

- `cargo test --locked --all-targets --all-features`: 308 tests passed on macOS.
- Extended the status concurrency test to release a selected claim between
  target lookup and inventory loading; both sections retain the original snapshot.
- Added release regression tests for replacement claims and removed inner
  boundaries, and verified explicit missing CLI targets never fall back to a
  valid current workspace.
- Reviewed open's transaction: locator lookup and ownership snapshot remain in
  the same read-only transaction, with the connection closed before launch.
- Existing removal collision and session forwarding tests passed.
- `openspec validate add-workspace-locator --strict --no-interactive`: passed.
- `git diff --check`: passed. Only the shared locator retains ancestor traversal.
- Commit hooks verify formatting, Clippy with warnings denied, all-target
  compilation, rust-analyzer diagnostics, Markdown, notices, and SQL boundaries.

No database migration or new dependency was introduced. The existing ID-only
open API remains a delegating compatibility wrapper. CLI claim text is parsed
in shared conversion to preserve the existing invalid-claim diagnostic.

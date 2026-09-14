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

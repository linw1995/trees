## Verification

Implemented against `origin/main` at `5ecd658`.

- All 343 tests passed with all workspace targets and features enabled.
- The addition integration suite contains 29 tests, including table-driven failure cases.
- Clippy passed with warnings denied. Commit hooks also run formatting, compilation,
  Rust Analyzer, documentation checks, and the runtime SQL boundary check.
- Strict OpenSpec validation passed. CLI help exposes all shared selectors.

## Coverage

| Contract | Evidence |
| --- | --- |
| Shared selectors and claims | Parser conflicts, exact targets, nearest removed boundary, explicit misses, stale claims, and idle automatic rejection |
| Existing work preservation | Local content, selected branches, changed commits, ignored files, repeated inputs, and stable worktree IDs |
| Source inputs | Local paths, registered names, ambiguous names, URL provisioning, offline rejection, and input order |
| Directory safety | Name collisions, symlink destinations, missing identities, locked moves, and nested registered workspaces |
| Atomic publication | Injected failures in worktree, workspace, and terminal events leave prior pool and claim intact |
| Recovery | Absent root, complete physical changes before publication, compensation decisions, changed leases, and repeated compensation failures |
| Content retention | New local and ignored files prevent automatic deletion and retain unresolved history |
| Consumers | Status, open, project preparation, release, removal, exact-set reuse, and existing destination pools |

## Recovery Boundary

If container creation succeeds but its ownership event cannot be committed,
recovery preserves the unverified container. The failure test verifies that it
is empty before removing it, then retries the addition successfully. Production
recovery reports the original, staging, and intended paths for inspection and
repair; it does not guess ownership or delete uncertain contents.

Compensated residual rows represent additions that were never successfully
published. Only those rows are removed after safe compensation; their full
identity and history remain in immutable events. Successful membership and
removal tombstones remain intact.

## Platform Scope

Tests ran on macOS. Linux and Windows were not executed in this workspace.
Pull request checks provide the separate Linux validation.

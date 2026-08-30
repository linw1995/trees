## Why

`trees create` provisions a workspace, but the current lifecycle has no
explicit ownership boundary or retention policy after creation. Multiple
coding tasks can use the same Git worktrees concurrently, while creating a new
record for the same path is rejected. Deleting and recreating the worktrees
would make reuse destructive and would lose the stable workspace identity
already used by Codex integration and lifecycle tracking.

An explicit checkin/checkout protocol makes an existing workspace a reusable
resource: an automatic request is keyed by its repository directories, one
caller holds the selected slot at a time, the caller receives a durable lease
identifier, and a workspace is returned to the pool only when its worktrees
are safe to hand to the next caller. A separate management mode distinguishes
workspaces that Trees may automatically reclaim from workspaces whose
retention remains a manual responsibility.

## What Changes

- Change automatic `trees create` to accept repository directories without a
  concrete workspace path, match the exact repository set to an idle pool
  slot, and return a checkout identifier for the selected workspace.
- Provision a new automatic workspace under the Trees-managed workspace root
  when no safe matching slot is available, then return it already checked out.
- Resolve a configurable managed `workspaces_dir` from a persistent Trees
  configuration, normalize it to an absolute path, and use the platform data
  directory only as the default while keeping SQLite state separate.
- Add configuration read/write support for `workspaces_dir` without making a
  concrete workspace path part of automatic allocation.
- Add `trees checkin <workspace-path> --checkout-id <checkout-id>` to release
  the claim after reconciliation; reuse the automatic create form with the
  existing checkout identifier to renew an allocation.
- Add `trees gc --older-than <duration> [--dry-run] [--yes] [--force]` to
  reclaim idle automatic workspaces while never selecting manual workspaces;
  report the number not currently checked out and confirm the normal
  destructive run. `--yes` skips only confirmation, while `--force` implies
  `--yes` and explicitly permits destructive cleanup of age-qualified unsafe
  automatic slots.
- Persist an `automatic` or `manual` workspace management mode inferred from
  the automatic repository-only or manual path-based command shape, together
  with the last successful checkin time used by GC.
- Persist at most one active checkout lease per workspace, with owner and
  expiry metadata, while keeping workspace health separate from access state.
- Require every managed worktree to be present, attached, detached, clean,
  and at its recorded revision before a lease can be acquired or released.
- Reclaim expired leases only after a successful safety check; never reclaim an
  unexpired lease and never reset or clean Git worktrees implicitly. Physical
  worktree removal is restricted to an explicit GC operation, with `--force`
  as the explicit opt-in for unsafe automatic-slot cleanup.
- Record checkout, renewal, checkin, rejection, and stale-lease recovery in
  the existing operation and immutable lifecycle event model.
- Preserve explicit-path creation for manual workspaces without adding a
  redundant mode flag, while keeping repair and manual-workspace overrides
  outside this change. GC retains lifecycle tombstones instead of deleting
  their database history.

## Capabilities

### New Capabilities

- `workspace-reuse`: Borrow and return an existing managed workspace through
  explicit checkout leases.

### Modified Capabilities

- `workspace-management`: Infer automatic/manual management from the
  repository-only versus explicit-path creation shape while preserving the
  direct-child worktree layout.
- `workspace-lifecycle`: Extend lifecycle persistence and reconciliation with
  management modes, active checkout leases, dirty/reclaimed states, and GC
  timestamps.

## Impact

- Extends the Clap command surface and dispatch in `src/cli.rs` and
  `src/main.rs`.
- Adds a lifecycle migration for management mode, active checkout leases, GC
  timestamps, and reclaimed states; Git observation also detects dirty
  worktrees.
- Extends the path/configuration layers with a configurable platform-specific
  automatic workspace root, absolute persisted paths, and generated workspace
  paths.
- Extends the typed domain, Diesel models, repository operations, and
  reconciliation workflow without introducing runtime raw SQL or a new
  database backend.
- Adds workspace reuse integration tests and concise CLI documentation.
- Does not change the physical direct-child worktree layout or automatically
  alter existing worktree contents.

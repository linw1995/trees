## Why

`trees create` provisions a workspace, but the current lifecycle has no
explicit ownership boundary or retention policy after creation. Multiple
coding tasks can use the same Git worktrees concurrently, while creating a new
record for the same path is rejected. Deleting and recreating the worktrees
would make reuse destructive and would lose the stable workspace identity
already used by Codex integration and lifecycle tracking.

An explicit acquire/release protocol makes an existing workspace reusable. An
automatic request is keyed by its repository directories. A caller holds the
selected slot through a persistent workspace claim. The workspace returns to
the pool only when its worktrees are safe to hand to the next caller. The claim
is a persistent ownership state for this workspace, not a long-lived SQLite
transaction or database lock. Operation leases, rather than workspace claims,
provide expiry metadata and renewal for automatic recovery when an operation
owner disappears. A separate management mode distinguishes workspaces that
Trees may automatically reclaim from workspaces whose retention remains a
manual responsibility.

## What Changes

- Change automatic `trees create` to accept repository directories without a
  concrete workspace path, match the exact repository set to an idle pool
  slot, and return a claim identifier for the selected workspace.
- Provision a new automatic workspace under the Trees-managed workspace root
  when no safe matching slot is available, then return it already claimed.
- Resolve a configurable managed `workspaces_dir` from a persistent Trees
  configuration, normalize it to an absolute path, and use the platform data
  directory only as the default while keeping SQLite state separate.
- Add configuration read/write support for `workspaces_dir` without making a
  concrete workspace path part of automatic allocation.
- Add `trees release <workspace-path> --claim-id <claim-id>` to release the
  claim after reconciliation.
- Add `trees gc --older-than <duration> [--dry-run] [--yes] [--force]` to
  reclaim idle automatic workspaces while never selecting manual workspaces;
  report the number currently unclaimed and confirm the normal
  destructive run. `--yes` skips only confirmation, while `--force` implies
  `--yes` and explicitly permits destructive cleanup of age-qualified unsafe
  automatic slots.
- Persist an `automatic` or `manual` workspace management mode inferred from
  the automatic repository-only or manual path-based command shape, together
  with the last successful release time used by GC.
- Persist at most one active workspace claim per workspace, with a claim
  identifier and timestamp, while keeping workspace health separate from access
  state.
  Claim and operation-lease updates SHALL use short SQLite transactions; Git
  and filesystem work SHALL never hold those transactions open.
- Require every managed worktree to be present, attached, detached, clean,
  and at its recorded revision before a claim can be acquired or released.
- Recover abandoned operations through operation-lease expiry and fresh
  reconciliation. Workspace claims remain active until explicit release and
  are never inferred from process liveness. Physical worktree removal is
  restricted to an explicit GC operation, with `--force` as the explicit
  opt-in for unsafe automatic-slot cleanup; active claims remain protected.
- Record acquire, release, rejection, and operation recovery actions in the
  existing immutable lifecycle event model. Operation facts are append-only;
  current operation leases are managed in a separate `operation_leases` table.
  Lease renewals update only that current lease row without appending an event
  for each renewal.
- Preserve explicit-path creation for manual workspaces without adding a
  redundant mode flag, while keeping repair and manual-workspace overrides
  outside this change. GC retains lifecycle tombstones instead of deleting
  their database history.

## Capabilities

### New Capabilities

- `workspace-reuse`: Acquire and release an existing managed workspace through
  explicit workspace claims.

### Modified Capabilities

- `workspace-management`: Infer automatic/manual management from the
  repository-only versus explicit-path creation shape while preserving the
  direct-child worktree structure.
- `workspace-lifecycle`: Extend lifecycle persistence and reconciliation with
  management modes, active workspace claims, operation leases, dirty/reclaimed
  states, and GC timestamps.

## Impact

- Extends the Clap command surface and dispatch in `src/cli.rs` and
  `src/main.rs`.
- Adds one lifecycle migration that builds the final workspace reuse schema
  from the existing lifecycle tables. Legacy workspace IDs and worktree
  observations are preserved, existing explicit-path workspaces become
  manual, and active workspace claims are stored without claim owner or
  expiry metadata. Operation facts remain append-only, while current lease
  ownership and expiry move to `operation_leases` and are renewed during long
  external steps.
- Extends the path/configuration layers with a configurable platform-specific
  automatic workspace root, absolute persisted paths, and generated workspace
  paths.
- Extends the typed domain, Diesel models, repository operations, and
  reconciliation workflow without introducing runtime raw SQL or a new
  database backend.
- Adds workspace reuse integration tests and concise CLI documentation.
- Does not change the physical direct-child worktree structure or automatically
  change existing worktree contents.

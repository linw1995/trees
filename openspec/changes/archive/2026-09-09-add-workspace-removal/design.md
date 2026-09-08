## Context

GC already owns the safety-sensitive physical removal implementation, including
repository identity checks, workspace containment, clean-worktree validation,
lease renewal, and tombstone persistence. Explicit cleanup differs only in
selection: it targets one stable workspace ID, accepts manual workspaces, and
does not use idle age.

## Goals / Non-Goals

**Goals:**

- Remove one known workspace without weakening lifecycle serialization.
- Support both management modes through explicit user selection.
- Share the established physical validation and removal implementation.
- Offer a read-only preflight and interactive confirmation.

**Non-Goals:**

- Delete lifecycle rows or events.
- Infer a target from a path or current directory.
- Reclassify workspace management mode.
- Break an active claim or take over an unexpired operation.

## Decisions

### Use the Remove Command with a Stable Id

The interface is `trees remove <workspace-id> [--dry-run] [--yes]
[--force]`. `remove` names the physical user action without implying that
audit history is deleted; the persisted terminal state remains `reclaimed`.
Stable IDs avoid path ambiguity and compose with
`trees status --view workspaces`.

### Make Explicit Selection Independent from Retention Policy

The command accepts automatic and manual workspaces because a stable-ID target
is an explicit administrative choice. It does not inspect creation or release
age. This does not change GC: automatic selection remains age-based and manual
workspaces remain excluded.

### Preserve Hard Safety Guards

A claim or unexpired operation rejects removal. An expired operation is
recovered before a new removal operation starts. Normal mode requires a ready,
clean, detached, identity-matched workspace with no unexpected content.
`--force` permits unhealthy snapshots, missing worktrees, dirty or changed
detached worktrees, and unexpected content, but does not bypass active claims,
unexpired operations, workspace-root containment, source repository identity,
worktree identity for present paths, or branch attachment.

The workflow persists operation intent before external mutation, reconciles
under the operation lease, renews the lease during removal, and records the
terminal tombstone only after physical removal succeeds.

### Keep Preflight Read-Only

`--dry-run` opens the lifecycle database read-only, resolves the target, and
runs the same physical validation without recovery, event writes, or external
mutation. It reports the target path and eligibility reason.

### Confirm Destructive Execution

Execution asks for confirmation unless `--yes` or `--force` is supplied.
Noninteractive execution without either flag fails. `--force` prints a warning
and implies confirmation because it explicitly selects the broader removal
policy.

## Risks / Trade-Offs

- [A manual workspace may contain valuable local state] → Require explicit ID,
  confirmation, and clean validation unless `--force` is supplied.
- [Preflight can become stale] → Rerun admission and physical validation under
  a persisted operation lease immediately before removal.
- [Physical removal can partially fail] → Preserve the existing failed
  operation and reconciliation behavior without writing a reclaimed tombstone.

## Migration Plan

No data migration is required. Rolling back removes the command while leaving
valid lifecycle events and reclaimed tombstones readable by existing code.

## Open Questions

None.

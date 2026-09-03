## Context

The existing workspace is a durable container whose direct children are Git
worktrees. `trees create` currently receives a concrete path, stores one
workspace row and one repo-worktree row per child, and the lifecycle database
already serializes non-terminal Git operations. Reconciliation can detect
missing or diverged worktrees, but it does not currently model an access
claim, whether a worktree contains local changes, or whether Trees may
reclaim the workspace. A workspace path is unique, so the automatic form
must allocate by repository set rather than ask the caller for a path.

The reuse protocol must preserve the stable workspace and repo-worktree
identities, avoid handing one physical directory to two callers, and avoid
silently destroying edits made by a previous caller. Git and SQLite still do
not share a transaction, so access transitions must use the existing intent,
short-transaction, and boundary-reconciliation discipline.

## Goals / Non-Goals

**Goals:**

- Make a previously created workspace explicitly borrowable and returnable.
- Distinguish `automatic` workspaces, which Trees may reclaim, from `manual`
  workspaces, which are never selected by GC.
- Enforce one active checkout lease per workspace across processes.
- Keep workspace health (`ready`, `degraded`, and so on) separate from access
  availability.
- Allow reuse only for a clean, attached, detached worktree set at the
  recorded revisions.
- Recover abandoned leases without giving a new caller dirty or diverged
  files.
- Reclaim only idle automatic workspaces after a caller-supplied age threshold,
  while retaining database tombstones and lifecycle events.
- Preserve the existing workspace identity, Git worktrees, lifecycle history,
  manual create behavior, and direct-child worktree layout.

**Non-Goals:**

- Arbitrary user-defined pool capacity, pool priorities, or cross-machine
  workspace allocation.
- Reclassifying an existing workspace through a separate administrative mode
  command; new workspaces declare their management mode at creation time.
- Resetting, cleaning, deleting, branching, rebasing, or refreshing a
  worktree during checkout or checkin.
- Adding a repair command, a status/history command, or a resident watcher.
- Making the existing Codex launcher implicitly acquire or release a lease;
  consumer integration can use the lease APIs in a follow-up change.

## Decisions

### Separate Management Mode, Health, and Access

The workspace row stores a `management_mode` of `automatic` or `manual`, while
`WorkspaceState` continues to represent physical and Git health. Access
availability is derived from an active row in a new `workspace_leases` table:
no row means unclaimed, and one row means checked out. A degraded workspace
can therefore be unclaimed without being eligible for checkout, and a manual
workspace can remain healthy without becoming a GC candidate. This keeps
management policy, health, and access as three independent dimensions.

New `trees create` calls without a positional workspace path are
`automatic`; calls with an explicit path are `manual`. The command shape is
the mode discriminator, so no redundant `--mode` flag is accepted. Explicit
paths opt out of automated allocation, checkout/checkin, and retention. Legacy
rows created by the old explicit-path command are backfilled as `manual`
because their intended retention policy cannot be inferred safely; a future
explicit adopt operation can opt such a row into a pool. GC remains opt-in per
invocation and requires an explicit age threshold.

The pool registry stores repository-set identity separately from workspace
slots:

- `workspace_pools.id`: the stable UUID used as `pool_key` by workspace rows;
- `workspace_pools.hash_key`: a BLAKE3 fingerprint used for indexed lookup;
- `workspace_pools.repositories_json`: the canonical sorted JSON array used for
  exact matching after the hash lookup;
- `workspace_pool_repositories`: the many-to-many relation between pools and
  origin repositories;
- `origin_repositories`: one row per Git common-directory identity, including
  the canonical source path shared by worktrees and pools.

The hash is intentionally not unique. A hash collision creates separate pool
rows and the canonical JSON comparison selects the correct one.

The lease table stores the current claim only:

- `id`: the UUID v7 checkout identifier;
- `workspace_id`: the unique workspace foreign key;
- `owner_id`: a local invocation identity for diagnostics;
- `checked_out_at`: the acquisition time;
- `lease_expires_at`: the finite expiry time;
- `last_heartbeat_at`: the last successful acquisition or renewal time.
- `pool_key` on an automatic workspace row: the UUID of its registry pool.
  Manual rows may leave this field null.
- `workspace_root` on an automatic workspace row: the absolute resolved
  `workspaces_dir` used as the pool namespace. Manual rows may leave this
  field null.
- `last_checked_in_at` on the workspace row: the last successful return time
  used as the idle-age anchor, falling back to `created_at` if it has never
  been checked in.

Completed claims are represented by immutable lifecycle events rather than
retained rows. The workspace and repo-worktree IDs never change when a lease
is reused.

### Resolve the Managed Workspace Root

`workspaces_dir` is a configurable Trees-level storage location, not a
workspace identity supplied by an automatic allocation request. The path and
configuration layers SHALL expose it as `managed_workspace_directory()` and
resolve it once per invocation. `trees config set workspaces-dir <path>` SHALL
normalize and persist the configured value as an absolute path; the optional
configuration file uses a `workspace.workspaces_dir` field. If no value is
configured, the first version SHALL use these platform defaults:

- Linux: `$XDG_DATA_HOME/trees/workspaces`, falling back to
  `~/.local/share/trees/workspaces`;
- macOS: `~/Library/Application Support/trees/workspaces`;
- Windows: `%LOCALAPPDATA%\\trees\\workspaces`.

The configuration file is located in the platform configuration directory
(`$XDG_CONFIG_HOME/trees/config.toml` or `~/.config/trees/config.toml` on
Linux, `~/Library/Application Support/trees/config.toml` on macOS, and
`%APPDATA%\\trees\\config.toml` on Windows). The existing `db.sqlite` remains
in the platform state directory. Trees SHALL create `workspaces_dir` lazily
and SHALL generate each automatic workspace as
`workspaces_dir/ws-<workspace-uuid>`. The generated child name is opaque and
does not encode repository paths or user input. A per-request concrete
workspace path is not part of automatic allocation.

Every persisted automatic workspace path and its persisted `workspace_root`
pool namespace SHALL be absolute. A configured root change SHALL affect only
future allocation; existing rows retain their absolute roots and are not
moved or rewritten automatically. Pool matching SHALL include the resolved
root namespace, so a workspace from a previous configured root is not silently
selected from a new root.

### Allocate Automatic Workspaces by Repository Set

The command contract separates automatic allocation from manual provisioning:

```text
trees create --repo <repository-path>...
trees create --repo <repository-path>... --checkout-id <checkout-id>
trees create <workspace-path> --repo <repository-path>...
trees checkin <workspace-path> --checkout-id <checkout-id>
```

The automatic form does not accept a concrete workspace path. It canonicalizes
and inspects every repository, derives a BLAKE3 hash and canonical JSON array
from the sorted set of Git common-directory identities, resolves the matching
pool registry UUID, and searches only `automatic` workspaces that reference
that pool. It filters out rows that are not `ready`, have an active operation
or lease, or fail the live reusable-worktree predicate.

If multiple candidates remain, Trees selects the least-recently-used slot by
`last_checked_in_at`, falls back to `created_at` for a never-used slot, and
uses the workspace UUID as a deterministic tie breaker. Lease acquisition is
the final atomic race check; if another process wins, allocation retries the
next candidate.

If no safe candidate exists, Trees generates a new workspace UUID and creates
the physical directory below the resolved `workspaces_dir`. The generated path
is not accepted from the caller. It initializes the same direct-child detached
worktrees as the existing create flow and acquires the caller's checkout lease
as part of the creation intent. A failed provisioning attempt is rolled back
at the filesystem level where possible and remains a failed lifecycle record
for diagnostics; it is never returned as an allocated workspace.

The optional existing checkout identifier renews the matching lease for the
same repository-set request and returns the same workspace. `checkin`
releases the lease. Manual provisioning requires an explicit path and bypasses
pool allocation, automated checkout, and GC; manual callers continue using
the existing workspace/Codex paths without automated claims.

The automatic command prints the allocated workspace path, repository-set
pool key, checkout identifier, and expiry so an orchestrator can persist the
allocation and pass the identifier to later renewal or checkin calls. The
identifier is a coordination token, not a security boundary; local filesystem
and database permissions remain authoritative.

### Require a Reusable Git Snapshot

Checkout and checkin both reconcile the workspace against Git. A worktree is
reusable only when all of the following are true:

- the source repository common directory still matches the persisted
  repository identity;
- the persisted worktree path exists and is still listed by the source
  repository;
- the worktree is not prunable and is a detached worktree;
- its `HEAD` equals the persisted `last_head`; and
- `git status --porcelain=v1 --untracked-files=all` reports no staged,
  unstaged, or untracked changes. Ignored files are not treated as edits.

The live cleanliness result is included in the reconciliation fingerprint.
The domain adds `dirty` to `RepoWorktreeState`; a dirty worktree is never
eligible, and the containing workspace becomes `degraded`. A simultaneous
branch, identity, or path mismatch remains `diverged` so the more serious
association error is not hidden by the cleanliness result.

This policy intentionally rejects committed changes made in a detached
worktree as well: a changed `HEAD` is `diverged` relative to the recorded
checkout baseline. The caller must repair or intentionally preserve such work
outside this change; Trees never runs `reset --hard`, `clean`, or worktree
removal as part of checkin.

### Acquire and Renew a Lease Atomically

For an existing pool candidate, Trees reconciles and verifies that the
workspace is automatic, `ready`, has no active operation, and has no active
unexpired checkout lease. The lease insert, allocation operation completion,
and `workspace_checked_out` event are committed in one short SQLite
transaction. The unique `workspace_id` constraint is the final race check, so
concurrent allocations cannot both succeed. For a newly provisioned slot,
the creation intent owns the generated workspace and lease while Git setup is
running; the lease is returned to the caller only after creation reaches
`ready`.

The default lease duration is 24 hours. Renewal requires the current
checkout identifier, runs as a short `checkout_renew` operation, and extends
the expiry by another 24 hours from the renewal time. Renewal does not reset
or otherwise mutate Git. It is allowed while the workspace is degraded so
the current owner can repair or recover its files without another caller
claiming them; checkin remains blocked until the workspace is reusable.

If post-acquisition reconciliation finds an external change, Trees releases
the new lease and records the failed checkout before returning an error. The
caller never receives a successful checkout result for a workspace that fails
the final safety check.

### Check In Without Destructive Cleanup

Checkin requires the workspace path and the active checkout identifier. Trees
holds the lease while it reconciles the worktrees. If the snapshot is clean
and matches the recorded detached revisions, a short transaction deletes the
active lease, completes the operation, and appends `workspace_checked_in`.
The physical workspace directory, Git worktrees, files, branches, and source
repositories remain untouched.

If reconciliation observes dirty, missing, prunable, diverged, or failed
worktrees, checkin records `workspace_checkin_rejected`, marks the workspace
degraded through the existing lifecycle transition, fails the checkin
operation, and keeps the active lease. Keeping the claim lets its owner fix
the workspace without exposing it to the next caller. A repeated checkin can
succeed after the owner has repaired the state externally and reconciliation
sees the original baseline again.

### Recover Only Expired Leases

An unexpired active lease is never overridden, even if its owner process is no
longer observable. When a later checkout sees an expired lease, it first
reconciles the workspace. If the workspace is reusable, one transaction
records `workspace_checkout_expired`, removes the old lease, creates the new
lease, and records `workspace_checkout_reclaimed`. If the workspace is not
reusable, the expired lease is removed, the workspace remains degraded, and
the checkout fails; no caller receives the path.

The operation and event details include the old and new checkout identifiers,
owner identities, expiry timestamps, and any reconciliation reason. This
makes stale-lease recovery auditable without retaining an obsolete current
lease row or attempting unreliable process-liveness detection. Automatic pool
allocation is the only operation that may turn a safely expired lease into a
new allocation; GC never overrides an unexpired lease.

### Reclaim Only Idle Automatic Workspaces

`trees gc --older-than <duration> [--dry-run] [--yes] [--force]` calculates a
cutoff from the current UTC time. A workspace is idle when its
`last_checked_in_at`, or `created_at` when it has never been checked in, is
strictly older than the cutoff. GC considers only `automatic` workspaces in
the current resolved workspace-root namespace. An expired lease is handled by
the safe lease-recovery rule first; an unexpired lease always skips the
candidate.

Before a non-dry-run GC starts, it prints a summary containing the number of
automatic workspaces, the number currently not checked out (no unexpired
checkout lease), the number currently checked out, the number matching the
age threshold, and the number that are safe to reclaim. It then asks for an
interactive confirmation such as `Reclaim N workspaces? [y/N]`. `--yes`
skips this confirmation but keeps the normal safety filter. `--force` implies
`--yes` and uses the forced safety policy below. A non-interactive invocation
without either flag fails before mutation and tells the caller to inspect with
`--dry-run`, use `--yes`, or explicitly use `--force`. A dry run prints the
same counts and candidate reasons without writing SQLite, Git, or the
filesystem.

Before any deletion, GC performs a final read-only safety check for every
managed worktree. Both modes must verify the source repository identity when
possible, the expected direct-child path, and that the target is within the
stored automatic workspace root. Without `--force`, the worktree must also be
detached, at the recorded `HEAD`, clean, present, and non-prunable; dirty,
missing, diverged, failed, manual, leased, young, or unexpected-content
workspaces are skipped and preserved.

`--force` bypasses the confirmation and permits cleanup of age-qualified
automatic workspaces that are dirty, diverged, missing, prunable, or contain
unexpected files. It may use `git worktree remove --force` and remove
unexpected content below the target automatic workspace root, so uncommitted
or untracked data can be destroyed. It SHALL still refuse manual workspaces,
unexpired checkout leases, active operations, young workspaces, and any path
whose source repository identity cannot be verified. `--force` does not
override the configured root-containment or repository-identity guards.

An executing GC creates one `gc` operation per candidate. It removes each
worktree with a non-forced `git worktree remove` in normal mode or the forced
variant when `--force` is set, then removes the empty workspace directory (or
the explicitly authorized unexpected content in forced mode). Only after all
physical removals succeed does it mark
repo-worktree rows and the workspace as `reclaimed`, finish the operation as
`succeeded`, and append a `workspace_reclaimed` event. The database rows and
all prior lifecycle events remain as tombstones, so a reclaimed workspace is
not reusable and its history is not lost. Recreating the same canonical path
is outside this change.

If a physical removal fails after an earlier removal succeeded, GC stops,
records the partial result and failure details, marks the workspace `failed`
or `degraded` as appropriate, and never reports a successful reclamation. It
does not attempt an unsafe automatic rollback or continue with additional
removal steps.

### Reuse Existing Lifecycle Transactions and Events

Checkout, renewal, checkin, stale-lease recovery, and GC are represented as
normal `operations` with kinds `checkout`, `checkout_renew`, `checkin`,
`checkout_reclaim`, and `gc`. Their intent is persisted before any lease or
reclamation mutation, and their terminal state, state change, and lifecycle
event are committed atomically in short Diesel transactions. Access and GC
events use
`entity_type = workspace` and the stable workspace ID; structured details
carry lease-specific identifiers, GC counts, age cutoffs, and the `forced`
marker when applicable.

Git reads happen outside SQLite transactions. Before returning from automatic
allocation or checkin, the workflow performs a final reconciliation so the
operation result is based on Git's authoritative metadata rather than a stale
database snapshot. Existing `trees codex` behavior remains backward compatible
and is not implicitly coupled to this lease in this change.

## Risks / Trade-Offs

- [A caller forgets to check in] → Use a finite 24-hour lease, explicit
  renewal, and safe expiry recovery; never reclaim an unexpired claim.
- [A caller leaves edits in a worktree] → Reject checkin, persist `dirty` or
  `degraded` state, and keep the lease so no data is discarded or shared.
- [The workspace changes between preflight and final verification] → Keep the
  lease held through the final reconciliation and roll back the claim when
  the post-check fails.
- [A repository-set key can fail to match a reusable slot] → Derive it from
  canonical Git common-directory identities, sort and serialize it
  deterministically, and treat a false negative as safe pool expansion rather
  than mutating an uncertain workspace.
- [GC could delete a user-owned workspace] → Persist `automatic` versus
  `manual`, select only automatic rows, require an explicit age threshold,
  skip all unsafe states, and retain tombstones.
- [GC can fail after a partial physical removal] → Use non-forced removal,
  preflight every worktree and root entry, record each step, and mark partial
  failure instead of claiming an atomic filesystem transaction.
- [A forced run can destroy local work] → Require the explicit `--force`
  flag, print the affected counts and warning, retain age/root/lease/identity
  guards, and record `forced: true` with the removal details.
- [The lease identifier is copied locally] → Treat it as coordination rather
  than authorization and rely on the existing local SQLite/file permissions.
- [Existing consumers do not pass a lease identifier] → Preserve their
  current behavior for compatibility and document that lease enforcement for
  every consumer is a follow-up integration boundary.

## Migration Plan

1. Add the workspace reuse migration and a follow-up normalization migration
   for management mode, GC timestamps, reclaimed states, the pool registry,
   origin repositories, pool relations, `workspace_leases`, and their indexes.
   Existing explicit-path workspace rows are backfilled as `manual` and start
   with no active lease; the migrations do not touch Git or delete files.
2. Extend the repository and domain layers without changing existing
   workspace or repo-worktree identifiers.
3. Make reconciliation understand `dirty` worktrees before enabling pool
   allocation, checkin, and GC.
4. Add the CLI workflow, safe non-forced GC removal path, and integration
   tests. If the change is rolled back, active claims must be released before
   removing the lease table; reclaimed filesystem content is not recoverable
   through Trees.

## Open Questions

- Should a future allocator enforce a maximum number of automatic slots per
  repository-set pool, or should GC remain the only capacity control?
- Should a future Codex wrapper own the checkout lease for the entire
  interactive process and renew it in the background, or should callers pass
  the identifier explicitly?
- Should a future administrative command reclassify existing workspaces
  between `automatic` and `manual`, or should that remain a migration-only
  policy?

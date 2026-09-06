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

- Make a previously created workspace explicitly reusable and returnable.
- Distinguish `automatic` workspaces, which Trees may reclaim, from `manual`
  workspaces, which are never selected by GC.
- Enforce one active workspace claim per workspace across processes.
- Keep workspace health (`ready`, `degraded`, and so on) separate from access
  availability.
- Allow reuse only for a clean, attached, detached worktree set at the
  recorded revisions.
- Recover abandoned operations without giving a new caller dirty or diverged
  files.
- Reclaim only idle automatic workspaces after a caller-supplied age threshold,
  while retaining database tombstones and lifecycle events.
- Preserve the existing workspace identity, Git worktrees, lifecycle history,
  manual creation behavior, and direct-child worktree structure.

**Non-Goals:**

- Arbitrary user-defined pool capacity, pool priorities, or cross-machine
  workspace allocation.
- Reclassifying an existing workspace through a separate administrative mode
  command; new workspaces declare their management mode at creation time.
- Resetting, cleaning, deleting, branching, rebasing, or refreshing a
  worktree during acquisition or release.
- Adding a repair command, a status/history command, or a resident watcher.
- Making the existing Codex launcher implicitly acquire or release a claim;
  consumer integration can use the claim APIs in a follow-up change.

## Decisions

### Separate Management Mode, Health, and Access

The workspace row stores a `management_mode` of `automatic` or `manual`, while
`WorkspaceState` continues to represent physical and Git health. Access
availability is derived from an active row in a new `workspace_claims` table:
no row means unclaimed, and one row means claimed. A degraded workspace
can therefore be unclaimed without being eligible for acquisition, and a
manual workspace can remain healthy without becoming a GC candidate. This
keeps management policy, health, and access as three independent dimensions.

The claim row is persistent usage state, not a long-lived SQLite lock. Claim
acquisition and release keep SQLite transactions limited to claim, snapshot,
operation-lease, and event changes. Git and filesystem work runs between those
short transactions so a slow subprocess cannot block other database users.
Operation facts are append-only. Current operation-lease state is stored in a
separate mutable table and may be renewed by its holder while external steps
run.

New `trees create` calls without a positional workspace path are
`automatic`; calls with an explicit path are `manual`. The command shape is
the mode discriminator, so no redundant `--mode` flag is accepted. Explicit
paths opt out of automated allocation, acquire/release, and retention. Legacy
rows created by the old explicit-path command are filled in as `manual`
because their intended retention policy cannot be inferred safely; a future
explicit adopt operation can opt such a row into a pool. GC remains opt-in per
invocation and requires an explicit age threshold.

The pool registry stores repository-set identity separately from workspace
slots:

- `workspace_pools.id`: the stable UUID for one logical repository set;
- `workspace_pools.hash_key`: a non-unique BLAKE3 lookup value derived from the
  sorted origin repository IDs; it is used only by the lookup index;
- `workspace_pools.repository_ids`: the canonical sorted origin repository ID
  set used for exact matching after the hash lookup; it is a collision-check
  payload, not a unique key or index;
- `workspace_pool_repositories`: the many-to-many relation between pools and
  origin repositories;
- `origin_repositories`: one row per Git common-directory identity, including
  the canonical source path shared by worktrees and pools.

The hash is intentionally not unique. A hash collision creates separate pool
rows and the exact `repository_ids` comparison selects the correct one. The
pool payload is not constrained as a unique key. A short immediate transaction
rechecks the exact payload before creating a missing pool, so concurrent
callers do not create duplicate logical pools without making the payload an
index.

The workspace claim table stores the current usage claim only:

- `id`: the UUID v7 claim identifier;
- `workspace_id`: the unique workspace foreign key;
- `claimed_at`: the acquisition time.

The claim remains while the caller uses the workspace, but it does not hold a
database transaction or database lock. It is released explicitly by the
release operation; this model does not infer abandonment from process
liveness or expire a claim. The workspace and repo-worktree IDs never change
when a claim is reused.

Operation facts and operation leases are deliberately separate:

- `operations` is append-only and stores the immutable operation identity,
  workspace, kind, intent, and start time;
- `operation_leases` stores at most one current lease per operation, references
  `operations.id` through a unique `operation_id` relation, carries the
  workspace ID with a uniqueness constraint, and uses its own `id` as the
  mutable lease token together with the expiry time;
- `lifecycle_events` stores append-only operation starts, steps, recoveries, and
  terminal transitions. The latest operation event is the operation state;
- Lease renewal updates only `operation_leases` and does not append a heartbeat
  event. Lease takeover replaces the lease token through an atomic
  lease-id/expiry check; `operation_id` is retained for reverse lookup.
- Lease-owned step and terminal persistence uses only the current `lease_id` as
  its mutable-state selector and derives `operation_id` from that lease row.
- When recovery proves that creation completed externally, the recovery
  terminal event, lease cleanup, ready snapshot, and workspace-ready event are
  committed in one finalization transaction.

The operation lease protects a mutation while Git or filesystem work runs
outside SQLite transactions. Workspace claims remain independent and are not
renewed or expired by this mechanism.

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

Every persisted automatic workspace path SHALL be absolute. A workspace slot's
root is derived as `canonical_path.parent()` and is not stored separately or
part of repository-set identity. A configured root change SHALL affect only
future slot creation; existing workspace rows retain their absolute paths and
are not moved or rewritten automatically. An idle workspace from a previous
configured root remains reusable when its repository set matches. GC derives
the slot root from each workspace path for containment checks.

### Allocate Automatic Workspaces by Repository Set

The command contract separates automatic allocation from manual provisioning:

```text
trees create --repo <repository-path>...
trees create <workspace-path> --repo <repository-path>...
trees release <workspace-path> --claim-id <claim-id>
```

The existing `--checkout-id` spelling may remain as a compatibility alias while
the claim terminology is introduced. The claim identifier applies to release,
not automatic creation.

The automatic form does not accept a concrete workspace path. It canonicalizes
and inspects every repository, resolves its `origin_repositories` rows, sorts
their IDs, derives a BLAKE3 hash and the exact `repository_ids` set, resolves
the matching pool registry UUID independent of workspace root, and searches
all `automatic` workspaces that reference that pool. It filters out rows that
are not `ready`, have an active operation or claim, or fail the live
reusable-worktree predicate.

If multiple candidates remain, Trees selects the least-recently-used slot by
`last_released_at`, falls back to `created_at` for a never-used slot, and
uses the workspace UUID as a deterministic tiebreaker. Claim acquisition is
the final atomic race check; if another process wins, allocation retries the
next candidate.

If no safe candidate exists, Trees generates a new workspace UUID and creates
the physical directory below the resolved `workspaces_dir`. The generated path
is not accepted from the caller. It initializes the same direct-child detached
worktrees as the existing create flow and acquires the caller's workspace claim
as part of the creation intent. A failed provisioning attempt is rolled back at
the filesystem level where possible and remains a failed lifecycle record for
diagnostics; it is never returned as an allocated workspace.

The automatic result returns the claim identifier for later release. The
release operation releases the claim only after reconciliation confirms that
the workspace is safe to reuse. Manual provisioning requires an explicit path
and bypasses pool allocation, automated claiming, and GC; manual callers continue using the
existing workspace/Codex paths without automated claims.

The automatic command prints the allocated workspace path, repository-set
pool ID, and claim identifier so an orchestrator can persist the allocation
and pass the identifier to later release calls. The
identifier is a coordination token, not a security boundary; local filesystem
and database permissions remain authoritative.

### Require a Reusable Git Snapshot

Acquire and release both reconcile the workspace against Git. A worktree is
reusable only when all these conditions are true:

- The source repository common directory still matches the persisted
  repository identity;
- The persisted worktree path exists and is still listed by the source
  repository;
- The worktree is not prunable and is a detached worktree;
- Its `HEAD` equals the persisted `last_head`; and
- `git status --porcelain=v1 --untracked-files=all` reports no staged,
  unstaged, or untracked changes. Ignored files are not treated as edits.

The live cleanliness result is included in the reconciliation fingerprint.
The domain adds `dirty` to `RepoWorktreeState`; a dirty worktree is never
eligible, and the containing workspace becomes `degraded`. A simultaneous
branch, identity, or path mismatch remains `diverged` so the more serious
association error is not hidden by the cleanliness result.

This policy intentionally rejects committed changes made in a detached
worktree as well: a changed `HEAD` is `diverged` relative to the recorded
acquire baseline. The caller must repair or intentionally preserve such work
outside this change; Trees never runs `reset --hard`, `clean`, or worktree
removal as part of release.

### Acquire and Release Workspace Claims

For an existing pool candidate, Trees reconciles and verifies that the
workspace is automatic, `ready`, has no active operation, and has no active
claim. The operation fact, current lease, and start event are committed in one
short SQLite transaction. Claim insertion and the `workspace_claimed` event are
then committed in another short transaction before the final reconciliation.
The operation remains lease-protected until that final check completes, so a
failed post-acquisition check can remove the new claim and append a terminal
failure without reopening a completed operation. For a newly provisioned slot,
the creation intent creates the workspace claim while Git setup is running;
each Git step is surrounded by short intent/result updates, and the claim is
returned to the caller only after creation reaches `ready`.

The claim remains in the database while the caller uses the workspace, but it
does not hold a database transaction or database lock. If post-acquisition
reconciliation finds an external change, Trees releases the new claim and
records the failed acquisition before returning an error. The caller never
receives a successful acquisition result for a workspace that fails the final
safety check.

### Release Without Destructive Cleanup

Release requires the workspace path and the active claim identifier. Trees
holds the claim while it reconciles the worktrees. If the snapshot is clean and
matches the recorded detached revisions, a short transaction deletes the
active claim, completes the operation, and appends `workspace_released`. The
physical workspace directory, Git worktrees, files, branches, and source
repositories remain untouched.

If reconciliation observes dirty, missing, prunable, diverged, or failed
worktrees, release records `workspace_release_rejected`, marks the workspace
degraded through the existing lifecycle transition, fails the release
operation, and keeps the active claim. Keeping the claim lets its holder fix
the workspace without exposing it to the next caller. A repeated release can
succeed after the claim holder has repaired the state externally and reconciliation
sees the original baseline again.

### Recover Expired Operation Leases

Workspace claims remain active until explicit release and are never replaced by
automatic allocation. Operation leases independently protect a mutation while
Git or filesystem work runs outside SQLite transactions. When an operation
lease expires, a later invocation may claim the operation through an atomic
lease-id/expiry check, observe external state, and either
finish or roll back the incomplete operation. Operation lease recovery does not
create, release, or extend a workspace claim.
Only a `create` operation may remove detached worktrees that it created during
an incomplete setup. Recovery of access, GC, or integration operations is
non-destructive: it reconciles current state and records a terminal recovery
event without removing a managed worktree.

### Reclaim Only Idle Automatic Workspaces

`trees gc --older-than <duration> [--dry-run] [--yes] [--force]` calculates a
cutoff from the current UTC time. A workspace is idle when its
`last_released_at`, or `created_at` when it has never been released, is
strictly older than the cutoff. GC considers all `automatic` workspaces and
uses each workspace path's derived parent root for containment checks. An
active claim or operation always skips the candidate. GC does not infer claim
abandonment from process liveness or override an active claim.

Before a non-dry-run GC starts, it prints a summary with the automatic,
unclaimed, claimed, age-qualified, and safe-to-reclaim counts. It
then asks for confirmation such as `Reclaim N workspaces? [y/N]`. `--yes`
skips this confirmation but keeps the normal safety filter. `--force` implies
`--yes` and uses the forced safety policy below. A noninteractive invocation
without either flag fails before mutation and tells the caller to inspect with
`--dry-run`, use `--yes`, or explicitly use `--force`. A dry run prints the
same counts and candidate reasons without writing SQLite, Git, or the
filesystem.

Before any deletion, GC performs a final read-only safety check for every
managed worktree. Both modes must verify the source repository identity when
possible, the expected direct-child path, that the target is within the
workspace path's derived parent root, and that the worktree is detached without
a branch. Without `--force`, the worktree must also be at the recorded `HEAD`,
clean, present, and non-prunable; dirty,
missing, diverged, failed, manual, claimed, young, or unexpected-content
workspaces are skipped and preserved.

`--force` bypasses the confirmation and permits removal of automatic
workspaces whose idle time is past the threshold, even when they are dirty,
diverged, missing, prunable, or contain unexpected files. It may use forced
Git worktree removal and remove unexpected content below the target workspace
path's derived parent root, so uncommitted
or untracked data can be destroyed. It SHALL still refuse manual workspaces,
active claims, active operations, young workspaces, and any path
whose source repository identity cannot be verified. `--force` does not
override the configured root-containment or repository-identity guards.

A GC run creates one `gc` operation per candidate. It removes each
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

Acquisition, release, and GC append immutable operation facts with kinds
`acquire`, `release`, and `gc`. Recovery reuses the expired operation fact and
only appends recovery lifecycle events; it never mutates or creates an
operation fact. Each intent is persisted before any claim or reclamation
mutation. The current lease row, terminal lifecycle event, state change, and
claim/reclamation mutation are committed atomically in short Diesel
transactions. External Git and filesystem work is performed between those
transactions. Access and GC events use `entity_type = workspace` and the stable
workspace ID; structured details carry claim identifiers, GC counts, age cutoffs,
and the `forced` marker when applicable.

Operation lease renewals are short lease-token-checked updates to
`operation_leases` made while an external step runs. They protect the in-flight
operation and do not hold SQLite transactions during Git or filesystem work.
Reconciliation keeps the caller's `operation_id` as lifecycle-event context;
it does not use that ID to select or mutate the current lease, which is always
addressed by `lease_id`. Git reads happen outside SQLite transactions. Before returning from automatic
acquisition or release, the workflow performs a final reconciliation so the
operation result is based on Git's authoritative metadata rather than a stale
database snapshot. Existing `trees codex` behavior remains backward compatible
and is not implicitly coupled to this claim in this change.

## Risks / Trade-Offs

- [A caller forgets to release] → Keep the active claim and require an
  explicit recovery action; do not infer abandonment from process liveness or
  silently hand the workspace to another caller.
- [A caller leaves edits in a worktree] → Reject release, persist `dirty` or
  `degraded` state, and keep the claim so no data is discarded or shared.
- [The workspace changes between preflight and final verification] → Keep the
  claim held through the final reconciliation and roll back the claim when
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
  flag, print the affected counts and warning, retain age/root/claim/identity
  guards, and record `forced: true` with the removal details.
- [The claim identifier is copied locally] → Treat it as coordination rather
  than authorization and rely on the existing local SQLite/file permissions.
- [Existing consumers do not pass a claim identifier] → Preserve their current
  behavior for compatibility and document that claim enforcement for every
  consumer is a follow-up integration boundary.

## Migration Plan

1. Consolidate the workspace reuse, origin, pool, claim, operation, and naming
   changes into migration `00000000000002`. The migration builds the final
   schema directly from the original lifecycle tables and preserves existing
   workspace IDs and repo-worktree observations. It drops workspace-claim
   owner, expiry, and heartbeat metadata, moves mutable operation lease state
   into `operation_leases`, and represents operation state transitions through
   append-only lifecycle events. It renames the idle timestamp to
   `last_released_at` and derives slot roots from workspace paths. Existing
   explicit-path workspace rows remain `manual` with no active claim; the
   migration does not touch Git or delete files.
2. Extend the repository and domain layers without changing existing
   workspace or repo-worktree identifiers.
3. Make reconciliation understand `dirty` worktrees before enabling pool
   acquisition, release, and GC.
4. Add the CLI workflow, safe non-forced GC removal path, and integration
   tests. If the change is rolled back, active claims must be released before
   removing the claim table; reclaimed filesystem content is not recoverable
   through Trees.

## Open Questions

- Should a future allocator enforce a maximum number of automatic slots per
  repository-set pool, or should GC remain the only capacity control?
- Should a future Codex wrapper own the workspace claim for the entire
  interactive process, or should callers pass the identifier explicitly?
- Should a future administrative command reclassify existing workspaces
  between `automatic` and `manual`, or should that remain a migration-only
  policy?

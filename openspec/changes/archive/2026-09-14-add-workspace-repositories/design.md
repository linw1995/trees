## Context

Baseline: `origin/main` at `5ecd658` (`feat(cli): share workspace locator and target selectors`,
PR #27). See `proposal.md` for Motivation. `src/naming.rs` Places a Single Repository At the Workspace
root and multiple repositories in named children. Consequently, adding the second repository
requires moving the original worktree, not merely creating another child inside it.

Pools identify exact sorted origin ID sets independently of workspace paths. Claims belong to
workspace IDs. Operations are immutable facts; current ownership lives in `operation_leases`, and
state and pending steps are represented by lifecycle events. Existing recovery treats all non-create
operations as failed after reconciliation, which cannot safely recover an addition that owns new
worktrees or a directory structure move.

## Goals / Non-Goals

**Goals:** Preserve user work, support both existing directory structures, maintain exact pool membership, and
make every admitted addition recoverable and auditable.

**Non-Goals:** Repository removal, branch selection, aliases, forced overwrite, implicit claims,
running-process path updates, a history command, or changing the create command's directory structure.

## Decisions

### Command and Admission

Use the existing top-level verb style and repeatable `--repo` arguments. Flatten the mainline
`WorkspaceLocatorArgs` into `AddArgs`, use the shared group with
`SelectionDefault::CurrentDirectory`, and convert the optional positional directory through
`WorkspaceLocatorInput::ExactPath`. Accept `--workspace-id`, `--workspace-dir`, and `--claim-id`;
all explicit selectors are mutually exclusive even when they identify the same workspace. Do not
overload positional paths with IDs.

```sh
trees add --repo web
trees add ./workspace --repo web
trees add --workspace-id WORKSPACE_ID --repo web
trees add --workspace-dir ./workspace --repo web
trees add --claim-id CLAIM_ID --repo web --json
```

Call the existing read-only `workspace_locator::locate` inside a caller-owned snapshot and keep
lifecycle eligibility, recovery, and admission in the addition service. An explicit miss never falls
back to current directory. Containing selection stops at the nearest registered boundary, including
an ineligible or removed workspace, rather than mutating an outer workspace. Preserve
`LocatedWorkspace.selected_claim_id`; for ID/path/current directory selection, read the active claim
in the same snapshot. Validate selector conflicts both through clap and direct argument conversion
before filesystem or storage access. Explicit ID/claim selection does not resolve current directory;
relative repository inputs may independently require a valid invocation directory.

Resolve relative repository inputs against the original invocation directory before any directory structure move.
The shared path resolver already canonicalizes an absent final component through its existing
parent, so a temporarily absent workspace root does not require a new locator fallback. ID and claim
selectors provide recovery paths independent of the target's filesystem presence. Do not change
locator behavior or existing command defaults.

Capture the active claim during selection and recheck the same claim during one-shot lease
admission. Manual workspaces need no claim; automatic workspaces require one. Reject idle automatic
workspaces rather than implicitly allocating them. Recover an expired operation before admission,
then check again target and claim. A busy attempt does not wait into a later operation slot.

Parse argument syntax before admission. Once the workspace and basic eligibility are established,
insert an immutable addition operation and lease in a short transaction before source provisioning
or target mutation. Later validation failures and no-op additions therefore have terminal audit
events. Unknown targets, invalid syntax, and failed admission do not create an addition operation.

### Plan Additions Independently of Creation

Introduce a dedicated `add` orchestration module and typed Snafu errors. Share origin input resolution
and revision selection rather than invoking create against an existing path. Split identity
resolution from fetch so an already-present origin is not fetched or aligned. Deduplicate both
aliases and repeated inputs by canonical Git common-directory identity, preserving first-seen order
for output.

Validate existing associations by identity and presence, without requiring clean or detached
worktrees. Dirty worktrees and user-selected branches or commits are preserved; observed health
remains truthful and can remain degraded. Missing, prunable, identity-mismatched, failed, or
unresolved partial associations reject addition. A no-op still verifies the requested association
rather than claiming success for an absent worktree.

Persist the complete execution plan in an append-only planning event before the first workspace
mutation. The initial operation intent contains a versioned request envelope because URL resolution
can create origin records after admission. The plan identifies origin IDs and identities, revisions,
new association IDs, target paths, old membership, old pool, proposed new repository set, captured
claim, and any directory structure move. Never update `operations.intent_json` in place.

### Promote a Single-Worktree Directory Structure Explicitly

Adding a distinct second repository moves the original worktree from `W` to `W/<source-name>`.
Existing multi-repository child paths never move. Validate all final names and paths first,
including collisions among new and existing repositories, symlinks, nested managed workspaces, and
source containment.

For root promotion, use an operation-owned unique sibling staging path on the same filesystem: move
the existing linked worktree from `W` to staging with Git, create an empty container at `W`, then
move staging to `W/<source-name>` with Git. Persist each move/`mkdir` intent and result; use exclusive
path creation and record ownership evidence. Resolve inputs before moving the original directory. Use absolute filesystem paths and run Git
from the source repository, without changing the process directory. Do not move a primary worktree, bypass worktree locks, or copy
worktree contents. If Git cannot safely move the worktree, including unsupported submodule directory structures,
fail without forcing it.

Retain the existing association ID, HEAD, index, local branch, tracked modifications, untracked
files, and ignored files. Root content follows the original worktree into its child. The workspace
path and claim remain stable, but a caller's shell can follow the moved directory and running tools
may retain stale absolute paths. Print old/new paths and document this behavior; do not promise live
integration refresh.

Alternatives rejected: nesting new repositories inside the original worktree changes its untracked
content and breaks the established directory structure. Rejecting every single-repository workspace excludes the
common expansion case. Changing all create directory structures broadens this feature into a compatibility
migration.

### Operation Journal and Publication Boundary

Use `kind = add` with a versioned intent payload. Keep current schema and existing lease/state
conventions. Store progress in immutable events with stable operation and association IDs. Suggested
event vocabulary:

| Event | Durable details |
| --- | --- |
| `operation_started` | Target, requested inputs, offline flag, mode, captured claim, old pool |
| `workspace_add_planned` | Resolved additions, existing inputs, revisions, path map, staging ownership, old/new repository sets |
| `operation_step_intent` | Step kind, source/target paths, association identity, expected preconditions |
| `worktree_relocated` | Original, staging, or final path, existing association ID |
| `worktree_added` | New association ID, origin ID, path and selected HEAD |
| `workspace_repositories_added` | Added and existing IDs, relocated paths, previous/current pool, claim |
| `operation_failed` / `operation_rolled_back` | Failure phase, source error details, completed steps, compensation results, and retained origins |
| `operation_recovered` | Observations, recovery decision and terminal outcome |

Use existing event helpers where their semantics match; event details supply the step type rather
than introducing mutable progress columns. Renew and verify the lease around long Git work,
including source provisioning. Never run Git or filesystem work inside a database transaction.

Keep new membership and relocated canonical paths in the journal until final publication. Under the
lease, inspect the planned final directory structure explicitly rather than applying ordinary reconciliation to
stale pre-promotion paths. At success, one short transaction inserts new associations, updates the
relocated association path, ensures the exact destination pool and its origin relations, changes
this workspace's pool reference, records observed health, appends successful workspace and operation
events, and removes the lease. Verify the captured claim remains unchanged. Manual mode and null
pool remain unchanged. Other workspace and pool memberships are untouched.

This avoids exposing a completed membership with an old pool. Read-only status may see the previous
snapshot plus a running operation while physical work is in progress; it must not present
uncommitted additions as completed. Output success only after publication. A lost standard output
response after commit is handled by an idempotent retry.

Each new target directory is created exclusively and its filesystem identity is recorded before
Git attaches a worktree. Recovery checks that evidence before publication or compensation. An
intended path alone cannot establish ownership. If directory creation succeeds but the identity
record fails, preserve the unverified directory for repair. Source publication events retain origin
IDs and paths even when a later clone fails before the complete plan is available.

### Compensation and Recovery

Use a dedicated `add` recovery dispatch before the generic non-create branch. Resolve expired adds
from operation intent plus journal, even when `W` is temporarily absent. Use the shared ID/claim
selectors or the existing exact-path normalization with an existing parent; perform recovery before
physical workspace eligibility checks, without adding filesystem-dependent behavior to the locator.

Ordinary execution failures before publication append failure details, durably select compensation,
remove only safe new worktrees in reverse order, and restore a promoted original worktree to `W`.
Journal every compensation before execution. Remove the replacement container only when ownership
and emptiness are proven. Never remove, reset, or recreate the original worktree. A completed
compensation ends `rolled_back`; pre-mutation rejection ends `failed`.

After interruption, inspect Git metadata and every recorded source, target, and staging location
under a newly claimed recovery lease. If the full intended directory structure exists, all new worktrees match
the recorded identity/revision and remain clean, and no compensation was selected, finish the original
publication transaction as succeeded. If only part exists, select compensation and restore the prior
directory structure. Once compensation is recorded, recovery continues compensation even if the desired directory structure happens to
exist. An interrupted operation with no mutation plan terminates failed without guessing ownership.

If cleanup cannot prove ownership, a new worktree acquired user changes, or directory structure restoration
fails, preserve files, append failure details, and retain observations of residual worktrees for
diagnosis. Persist residual associations as failed and mark the workspace degraded, while leaving
its original pool and claim unchanged. Such an unresolved addition blocks subsequent mutations that
could publish or release inconsistent membership; retrying `add` first attempts the recorded recovery,
and unsafe residual state returns an actionable repair error. A terminal failed `add` is not reopened:
a retry that finds unresolved residuals acquires a new recovery operation linked to the original
operation ID and journal, attempts only its recorded compensation, and preserves both histories.
Record the recovery linkage and resolution in immutable events; membership guards must inspect
unresolved addition journals, not only workspace health. If compensation still cannot finish,
release the recovery lease with failure and report the repair details. Explicit removal retains its
existing safety policy. Exclude residual failed/removed associations from successful repository-set
calculations. Successfully compensated residual rows are deleted only when they represent unpublished additions.
Their identities, paths, and compensation history remain in immutable events. Successful membership
and removal tombstones are never deleted. This respects existing unique membership constraints
without introducing a migration.

Source cloning retains the existing independent reservation and recovery mechanism. Published
origins survive workspace compensation and are reported on standard error; fetch side effects are not
rolled back. Add must not claim complete external compensation merely because its workspace directory structure was
restored.

### Result and Integration Contract

Default output is line-oriented key/value text identifying the workspace and operation,
current/previous pool, claim, per-repository result, and relocated paths. `--json` returns one
object with `schema_version: 1`, `operation_id`, `workspace_id`, `workspace_path`, nullable
`claim_id`, nullable `previous_pool_id` and `pool_id`, `repositories` entries
(`origin_repository_id`, `worktree_id`, `worktree_path`, `result` equal to `added` or
`already_present`), and `relocated` entries (`worktree_id`, `previous_path`, `worktree_path`).
Diagnostics go to standard error. Success, including an all-existing request, exits zero; failure
does not emit a success object.

Subsequent status, release, reuse, removal, open, and project preparation must use committed
membership and paths. Existing project synchronization remains on its normal invocation boundary;
adding repositories does not launch or rewrite running sessions.

## Risks / Trade-Offs

- [Directory Structure promotion changes an existing repository path] -> Report the mapping, document shell/editor behavior, and test dirty and ignored content preservation.
- [Filesystem and SQLite cannot commit atomically] -> Persist intent before each mutation, publish membership once, and test interruption at every boundary.
- [External processes can modify files despite the lease] -> Check again identities and cleanliness of new worktrees before deletion; preserve uncertain content.
- [Pool IDs returned by create become stale] -> Return old/new pool IDs; preserve claim-based release and document that pool membership can change.
- [Older binaries do not understand addition recovery] -> Complete or recover all pending additions before downgrading.

## Migration Plan

No new table or column is planned. Amend the existing lease-takeover specification to permit
`add`-owned compensation: the current blanket prohibition on non-create recovery removing worktrees
otherwise conflicts with this feature. Preserve that prohibition for access, integration, and
removal recovery. Add versioned journal payloads and recovery dispatch first, followed by
orchestration and CLI. Do not enable the command before recovery and compensation tests pass. Existing
workspaces are upgraded lazily only when a distinct repository is added. Validate release and
exact-set reuse after promotion and pool migration. Before downgrading, recover unfinished
additions; fully completed multi-repository workspaces retain the existing schema and directory structure.

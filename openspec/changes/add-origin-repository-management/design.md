## Context

`origin_repositories` already stores stable IDs, canonical Git common-directory
identities, and primary paths. Pool membership and repo-worktrees reference
those IDs. Create performs implicit registration; status already supports
read-only relational snapshots; remove currently selects a workspace UUID.
See `proposal.md` for the intended CLI changes.

## Goals / Non-Goals

**Goals:**

- Integrate repo management into create, status, remove, and configuration.
- Preserve origin identity and distinguish explicit ownership from location.
- Make repeated URL-based creation reuse a source clone and its workspace pool.
- Recover interrupted cloning without exposing partial origins or deleting
  successfully published or user-owned data.

**Non-Goals:**

- Dedicated repo commands, aliases, renaming, or independent registration CLI.
- Physical deletion of successfully registered origins or repository GC.
- Bare or unborn repositories, relocation, and automatic mode conversion.
- Equating different remote URL spellings or deduplicating manual clones by URL.

## Decisions

### Extend Existing Commands

```text
trees create [WORKSPACE_PATH] --repo <PATH|URL|NAME>...
trees status --view repos [--all] [--json]
trees remove <WORKSPACE_OR_REPO_ID> [--dry-run] [--yes] [--force]
trees config set origins-dir <PATH>
```

NAME searches registered origins by exact primary source directory base name,
including both automatic and manual origins. It reuses the recorded source
and never provisions a new clone merely because a name was supplied.

There is no separate repo command group. A local path registers a new origin
as manual; a remote URL provisions an automatic origin if no matching automatic
origin exists. Repo and workspace modes are independent: either repo mode can
serve a workspace of either mode. Registration happens as part of create.

Use each primary source directory base name as its name; do not store an alias
or enforce global base name uniqueness. Reuse the status shortest-unique-path
suffix renderer for colliding display names. Stable IDs remain the targets for
remove, so directory names cannot make removal ambiguous.

### Classify Inputs Before Side Effects

Recognize explicit absolute paths, `./` and `../` paths, and Windows drive paths
as paths first. Recognize supported URI forms (`https://`, `http://`, `ssh://`,
`git://`, `file://`) and `SCP`-style `[user@]host:path` as URLs; callers can prefix
an unusual local colon-bearing path with `./`. Other values are local paths.
If a single-component relative path does not exist, resolve it against
registered primary directory base names: exactly one match is required. An
existing local directory takes precedence over the catalog. Missing paths
with components fail rather than falling back to a name. No UUID or alias
interpretation is added to create.

Parse every input and validate all local inputs, catalog selections, duplicate
URLs, and predictable directory collisions before network or workspace mutation.
Reject an unresolved URL with `--offline` before any clone; an already known,
valid automatic clone remains usable offline with the existing local HEAD
rule. After URL resolution, deduplicate by canonical Git common directory and
run existing create validation, including collisions discovered after resolving
primary paths. Inputs that fail late do not yield a partial workspace.

Carry resolved identities into the workspace plan and validate again them before
using the recorded source. Name resolution binds an in-flight request to an
identity, not a later owner of the same base name.

### Record Mode and Ownership Without a Name Registry

Add `registered` (default true), `management_mode` (default manual), nullable
`managed_root`, and nullable `remote_url` to origin rows. Automatic origins
require recorded root and provisioning URL; manual origins have neither.
Existing rows migrate as registered and manual, irrespective of containment.
Retain IDs, common-directory uniqueness, source paths, and pool keys.

A path under `origins-dir` is still manual when first supplied as a user checkout.
Resolving an existing automatic origin through its path or linked worktree
preserves its automatic metadata. Neither path containment nor the current
configuration transfers ownership. Registration Removal and re-registration preserve mode.
Runtime storage operations use typed Snafu errors and short transactions.

### Reuse Automatic Origins by Exact Provisioning URL

Store the accepted provisioning URL as an exact, case-sensitive lookup key for
automatic origins, with a uniqueness constraint across retained automatic rows.
Use one record per exact URL across root configuration changes. Do not normalize
SSH and HTTPS URLs into one identity, inspect manual remotes for adoption, or
merge distinct local Git common directories. URL lookup is a provisioning
convenience; pool identity still uses canonical local origin IDs.

A known URL resolves to the retained automatic origin after validating its
stored source and common directory. An unregistered retained clone can be
registered again with the same ID. A missing or replaced recorded clone fails
with its ID and path; do not silently replace an identity referenced by history.
Changing roots does not move or duplicate a known clone. Different URL spellings
may provision separate clones. Authentication uses the existing Git environment;
URLs with embedded secrets must not be persisted or echoed unredacted.

### Configure and Allocate Automatic Storage

Store `repository.origins_dir` in existing TOML configuration. Default to
`trees/origins` under the platform data base. Resolve relative configured paths
against the configuration file directory, as existing workspace configuration does.
Setting it preserves unrelated keys and neither clones nor moves files.

At provisioning, snapshot and canonicalize the effective root. Allocate an
exclusive UUID container below it and clone into a child with the sanitized
remote base name (strip trailing `.git`, fallback `repo`). For example, a URL
ending in `api.git` yields `<root>/<uuid>/api`, so the repo directory name is
`api`. Existing content is never overwritten or adopted. Persist the canonical
root on the automatic origin. Manual registration never creates the root.

### Persist Clone Intent and Recover Interrupted Provisioning

Before cloning, reserve a pending operation containing its UUID, exact URL,
root, and target. Serialize reservation/publication for the same URL and hold
an exclusive process-owned lock during Git work. A concurrent request for a
live reservation fails with an in-progress diagnostic and can retry; it must
not allocate a second clone. Do not hold a database transaction across Git.

Create the UUID container exclusively and retain ownership evidence. Invoke
Git directly with separated arguments, an option terminator, and an absolute
destination. Do not run a shell or initialize submodules recursively. Keep
progress on standard error. Validate a usable primary HEAD and common directory, then
publish the automatic origin and consume the pending reservation atomically.
A newly cloned source can continue through existing create revision selection,
including its normal fetch behavior; no separate revision policy is introduced.

On clone, validation, or publication failure, remove only the provably owned
partial container inside its recorded root. Check whether publication committed
before cleanup after an uncertain database result. Retain committed origins.
A subsequent create that encounters an abandoned URL reservation acquires its
operation lock and recovers it before retrying. Unknown ownership or failed
cleanup retains the record and reports the operation ID. It does not block
unrelated URLs. Status and remove preflight never perform clone recovery.

Origin provisioning commits independently of workspace creation. If a later
URL or workspace operation fails, successfully registered origins remain for
retry; partial worktrees follow existing workspace rollback. Report retained
origin IDs on standard error. This avoids deleting a source already acquired by another
request and makes repeated create reuse successful work. Pending origins never
appear as registered or become available for selection through directory-name lookup.

### Add Repos to the Existing Status Snapshot

Reuse status's read-only transaction, timestamp, missing-database handling,
control-character escaping, and version-1 JSON envelope. Default remains pools;
`--all` is accepted for repos and workspaces, not pools. Repos shows registered
rows by default and retained unregistered rows with `--all`.

Human columns are `REPO`, `MODE`, `STATUS`, `PATH`, and `ID`, ordered by stored
source path then ID. Mode uses existing automatic/manual icons. Status reports
registered/unregistered metadata, never live Git health. JSON uses `view: repos`
and a `repos` array with `origin_repository_id`, `source_path`, `label`,
`repository_identity`, `management_mode`, `registered`, `managed_root`, and
`remote_url`. Keep pools/workspaces JSON contracts unchanged. Older schemas
produce an upgrade error for repos without performing migration.

### Dispatch Remove by Stable Id

Resolve the supplied full UUID against workspace and origin tables in one
snapshot. Exactly one entity must match; unknown or cross-table ambiguous IDs
fail without mutation. Retain UUID-v7 input validation and existing workspace
removal behavior. `--dry-run` reports entity type, path, and planned action
using read-only storage.

For an origin, remove removes registration for metadata only in both modes. Preserve files,
origin row, ownership, pool relations, worktrees, claims, and history. Existing
workspace references therefore do not block registration removal. Reuse existing
confirmation handling; clearly identify `Remove Registration for repository` in the prompt
and result. `--yes` or `--force` bypasses confirmation, but force does not expand
repo removal into filesystem deletion. Already unregistered IDs succeed as a
no-op. Actual mutation rechecks target identity inside its short transaction.

A concurrent create can re-register an origin it has already resolved. Existing
workspaces continue to release or remove using retained identity regardless of
registration state. Physical repo deletion requires a separate future contract
for dependent worktrees and local Git data.

## Risks / Trade-Offs

- [Paths and URLs overlap syntactically] → Document deterministic parsing and
  explicit `./` paths; never interpret a failed URL as a local path.
- [Directory names collide] → Unique suffixes for display, reject ambiguous
  name lookup, and use full IDs for remove.
- [A failed workspace create leaves successful clones] → Preserve them as
  reusable registered origins and report their IDs rather than hiding them.
- [Exact URL reuse does not merge URL variants] → Keep lookup predictable and
  preserve local identity rather than guessing remote equivalence.
- [Remove suggests physical deletion] → Display entity type and action before
  confirmation; automatic origins are not physically deleted in this change.

## Migration Plan

1. Add mode, registration, ownership, URL uniqueness, and pending-clone schema;
   backfill legacy rows as manual without changing IDs or references.
2. Implement configuration, input resolution, cloning, and recovery; connect to
   both existing create modes before extending status and remove.
3. Verify legacy create/reuse/status/remove behavior, migration integrity, and
   the new end-to-end path and URL flows.
4. Resolve pending clone operations before an explicit down migration. Preserve
   origin IDs and workspace references; removing new metadata loses ownership
   and URL lookup information. Older binaries must not run concurrently with
   new provisioning because they do not honor its operation reservations.

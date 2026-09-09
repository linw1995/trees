## Context

The existing origin table already provides `id`, `repository_identity`, and
`source_path`. Worktrees and pool membership reference those IDs. Git stores
remote configuration, and source paths identify current repository locations.

## Goals / Non-Goals

**Goals:**

- Reuse existing origin records without new persistent repository attributes.
- Treat URL cloning and local registration as input strategies.
- Preserve workspace behavior and references when managing source records.

**Non-Goals:**

- Repository modes, aliases, soft deletion, source-file deletion, or origin GC.
- Cached copies of Git remote configuration or remote URL equivalence rules.
- Deleting historical workspace or pool relationships to permit origin removal.

## Decisions

### Use Existing Origin Identity

Do not add `registered`, `management_mode`, `managed_root`, or `remote_url` to
`origin_repositories`. Do not replace them with a nullable clone URL. Registration
remains an upsert by canonical Git common directory. Directory names are derived
from primary source paths. A local source can be inside the configured clone
root without gaining special lifecycle behavior.

### Resolve Inputs from Paths and Git Metadata

```text
trees create [WORKSPACE_PATH] --repo <PATH|URL|NAME>...
trees config set origins-dir <PATH>
trees status --view repos [--json]
trees remove <WORKSPACE_OR_REPO_ID> [--dry-run] [--yes] [--force]
```

Keep explicit path and supported URL classification. Existing relative paths
win over bare directory-name lookup. A name must match exactly one stored
primary directory base name; unknown or ambiguous names fail.

Capture origin rows in one database snapshot for input resolution. URL lookup
locally inspects readable origins with matching recorded Git identity and reads
Git `remote.origin.url`. An exact match reuses that source regardless of how it
was created or where it is stored. Multiple matches fail with candidate paths.
Missing or identity-mismatched sources are not URL matches; they remain visible
in status. Unreadable Git configuration on an otherwise valid source reports
an error. Remote changes take effect on the next lookup; no URL is cached in
origin rows. Status never invokes this Git inspection.

Unknown URLs clone into `<origins-dir>/<operation-id>/<directory-name>`, using
a sanitized remote base name. Known sources remain usable after root changes.
`--offline` rejects URLs that need cloning, but permits local matching sources.
Reject predictable invalid inputs, duplicate identities, and layout conflicts
before workspace mutation. Recheck selected identities before fetching.

### Keep Only Transient Clone State

The pending-clone table records operation ID, URL, root, destination, and an
ownership token before filesystem mutation. These fields are operation intent,
not repository attributes, and the row is consumed on successful publication.
URL locks serialize concurrent clone requests; after acquiring the lock, query
current origins and Git again before allocating a destination. Existing source
lookup does not depend on the configured root.

Keep exclusive destination creation, ownership validation, partial-clone
cleanup, and interrupted-operation recovery. A retry for a pending URL acquires
the operation lock before recovery. A source already published is never deleted
by cleanup. If a later workspace operation fails, successful sources remain
available for retry and their IDs are reported. Failures to prove ownership
retain pending evidence and do not remove unrelated files.

### Keep Status Strictly Read-Only

The repos human view contains `REPO`, `PATH`, and `ID`. Use shortest unique path
suffixes for conflicting labels and existing terminal escaping. The version-1
JSON envelope contains `view: repos`, `snapshot_at`, and a `repos` array with
`origin_repository_id`, `source_path`, `repository_identity`, and `label`.
There is no mode or registration filter. `--all` remains valid only for the
workspace view. Existing origin rows can be listed before the clone-operation
migration is applied. No schema upgrade or Git probe occurs during status.

### Remove Records Only Without References

Resolve full IDs against workspace and origin rows, rejecting cross-table
ambiguity. Workspace removal is unchanged. For origins, read-only preflight
reports the record, planned record-removal action, and counts of referencing
repo-worktrees and pool memberships. All references count, including reclaimed
worktrees and retained pools, because their history depends on the origin ID.

Only a record with zero references can be deleted. Repeat both entity resolution
and reference checks inside the write transaction; foreign keys provide the
final guard against concurrent workspace creation. `--force` skips confirmation
but never bypasses references or authorizes source-file deletion. Deleting the
record makes its ID unknown; later registration of the same surviving source
creates a new ID. There is no hidden or removed origin state.

### Migrate Only Clone Operations

This unreleased branch changes migration 3 to create only
`pending_origin_clones`. It does not alter existing origin rows or their IDs.
A rollback requires pending recovery first and drops only the operation table.
The previously committed migration has not been published; no history rewrite
is required. Databases already used with the intermediate development build
may retain ignored extra columns; this change does not run destructive schema
cleanup on user databases. Fresh and existing pre-feature databases keep the
original three-column origin table.

## Risks / Trade-Offs

- [Git URL lookup costs local inspection] → Read one catalog snapshot, perform
  no network operations, and leave status independent of Git availability.
- [Several checkouts share a remote] → Require an explicit path or unique name.
- [Historical references prevent record deletion] → Report reference counts;
  retain history instead of silently breaking relations.
- [A missing source no longer has a recoverable URL index] → Preserve its row;
  URL creation can allocate a new source without replacing the old identity.

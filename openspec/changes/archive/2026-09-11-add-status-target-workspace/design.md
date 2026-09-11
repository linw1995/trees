## Context

See [the proposal](proposal.md) for motivation and scope. Status currently has
three independent CLI loading paths and three snapshot envelopes. Each loader
starts its own read-only transaction. Workspace records already contain all
summary data, including claims, retained operations, repository states, and
reconciliation time. Existing human tables use compact UTC times.

The preceding terminology correction established `removed`, `removed_at`, and
status JSON schema version 2. This change builds on that contract.

## Goals / Non-Goals

**Goals:** Keep target selection independent from inventory selection, share
snapshot assembly and presentation helpers, and ensure the two sections cannot
contradict each other because of a concurrent lifecycle transition.

**Non-Goals:** Change global inventory columns or ordering, refresh persisted
health, filter the inventory by target ID, or add workspace admission checks.

## Decisions

### Resolve Workspace Targets

Add `workspace_id: Option<WorkspaceId>` to status arguments, matching `open`.
An explicit ID wins and does not require a working target directory. Without
an ID, resolve the process directory canonically and select the closest stored
workspace path equal to it or one of its ancestors. Compare path components,
not string prefixes. Include removed records in target lookup independently
of the inventory filter. The closest registered boundary wins even when it is
removed; do not accidentally select an outer workspace instead.

Canonicalizing the invocation directory is the only new filesystem observation.
Do not canonicalize stored target paths, inspect their contents, or run Git.
Resolve current-directory errors as typed errors; only a successful lookup
with no containing workspace is silent. An unknown explicit ID is an error,
including when no database exists. Load the entire result before writing output.

A `--id` flag would duplicate the convention already used by `open`. Restricting
lookup to exact current directory equality would miss normal execution from repository subtrees.

### Give the Combined Snapshot One Transaction Owner

Move orchestration into one status snapshot entry point. Start one read-only
transaction, capture one `snapshot_at`, select the target, and load the selected
inventory and target relationships within that transaction. Refactor existing
loaders into transaction-local helpers; do not call independent public loaders
and combine their results after they commit.

Reuse `WorkspaceStatus` for the target. Load only the target's related rows for
pool and repo views; reuse the corresponding workspace object when already in
the workspace view. Do not load every workspace's repository details just to
show one target. Keep relation loading inside the storage layer and use typed
Snafu errors for selection, query, path, time, and serialization failures.

A second query after rendering the view is simpler but can report inconsistent
claims and pool capacity. A separate detail-only view would not meet the
requirement to show the target with every existing view.

### Render a Compact Summary Before the Existing Table

The summary field order is ID, Path, Status, Mode, optional Operation, Repos,
and Reconciled. Use the existing claim marker and repository label semantics.
Use full stable IDs. Do not show a separate Claim row or extra pool identifiers.

```text
Workspace (current directory)
  ID          019...
  Path        /home/user/workspaces/backend
  Status      ready 🔒
  Mode        automatic 🤖
  Repos       2/2 api,web
  Reconciled  2026-09-11 14:32:05

Pools
REPOS    CAPACITY  UPDATED
api,web  2/3/0     14:32
```

An explicit ID changes only the summary heading to
`Workspace (selected by ID)`. Manual mode is `manual 👤`. A retained operation
adds a line such as `Operation   release / running (lease expired)`; always
show lease classification, including active and inconsistent leases. If no
operation exists, omit the row.

When a summary exists, separate it from the inventory with one blank line and
label the inventory `Pools`, `Workspaces`, or `Repositories`. When no target
exists, render exactly the existing inventory output without a heading, empty
summary, extra blank line, or diagnostic. Keep a target's row in the workspace
inventory if the existing filter includes it.

Escape all path and operation text before adding generated ANSI sequences.
Reuse display-width alignment, repository readiness colors, and `NO_COLOR`
behavior. Emoji remain in plain output, consistent with the existing tables.

Format the summary reconciliation instant in the platform's local timezone as
`YYYY-MM-DD HH:MM:SS`, without an offset or timezone name; use `never` for a
missing instant. Resolve the offset for that instant so daylight-saving changes
are handled correctly. Keep conversion injectable for deterministic tests. If
local timezone information is unavailable, use UTC without adding a suffix.
Existing inventory compact times and JSON RFC 3339 timestamps remain unchanged.

### Extend the JSON Envelope

Every view adds `target_workspace`, containing the complete existing
`WorkspaceStatus` object or null. Preserve `schema_version: 2`, `view`,
`snapshot_at`, and the selected `pools`, `workspaces`, or `repos` array.
The field name covers both current directory and ID selection, unlike `current_workspace`.
Claim and operation objects remain structured in JSON. Human headings and emoji
are never embedded as JSON metadata. A removed target remains present even
when omitted from the global workspace array without `--all`.

The addition does not change existing fields or types, so no new JSON version
is required. A wrapper that nests the existing envelope would break all current
JSON paths and is unnecessary.

## Risks and Tradeoffs

- Persisted health can be stale: retain the reconciliation timestamp and document
  that status does not report live Git cleanliness.
- Symlinks and nested registrations can select an unexpected outer workspace:
  canonicalize only current directory and select the nearest component boundary.
- Summary and inventory can drift during concurrent writes: keep one transaction
  and timestamp for all relationships and lease classification.
- Added human lines affect scripts parsing terminal tables: preserve JSON as the
  machine-readable interface and keep no-target output unchanged.
- Local timezone lookup varies by platform: isolate conversion and test a
  non-UTC offset, a date boundary, and the UTC fallback explicitly.
- The archived origin-management change added repos-view requirements to the
  main spec: preserve those requirements when syncing this delta.

## Migration Plan

No database migration is needed. Implement the selector and shared snapshot
first, then JSON and human rendering, then CLI regression coverage and docs.
Reverting this feature removes the new summary and additive JSON field without
changing persisted lifecycle data. Keep the preceding removal terminology fix.

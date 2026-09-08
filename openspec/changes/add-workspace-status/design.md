## Context

The lifecycle database already contains workspace snapshots, active claims,
operation facts and leases, latest operation events, and repo-worktree
snapshots. Those records deliberately separate health, usage, and mutation
ownership. A status command needs to preserve that separation and must not
turn an inspection into an operation boundary with recovery side effects.

## Goals / Non-Goals

**Goals:**

- Provide one fast inventory of all current managed workspaces.
- Make health, usage, operation activity, and observation freshness separately
  visible.
- Provide deterministic human output and a versioned machine-readable form.
- Read a transaction-consistent SQLite snapshot without changing external
  or persisted state.

**Non-Goals:**

- Reconcile persisted state with Git or the filesystem.
- Assert that an unclaimed workspace is currently reusable.
- Show lifecycle history or completed operations.
- Add interactive watching, filtering, sorting options, or a single-workspace
  selector in the first version.
- Replace `gc --dry-run` as the authoritative reclamation preview.

## Decisions

### Use a Top-Level Status Command

The interface is `trees status [--all] [--json]`. With no options, it lists all
non-reclaimed manual and automatic workspaces. `--all` also includes
reclaimed tombstones. An empty or not-yet-created lifecycle database is a
successful empty result; an unreadable, corrupt, or incompatible database is
an error.

The command intentionally has no positional default tied to the current
directory. Its primary purpose is a global inventory, and implicit current
workspace selection would make the same invocation change meaning based on
the caller's directory. A future detail command or selector can be added
without changing this contract.

### Keep Health, Usage, and Operation State Orthogonal

Each projection retains the persisted workspace health state and management
mode. Usage is derived only from active claim presence and is rendered as
`claimed` or `unclaimed`. Current operation information is derived from the
operation lease, its operation fact, its latest operation event, and the
snapshot time.

An operation lease is `active` when its latest operation state is `running` and
its expiry is later than the snapshot time. It is `expired` when the latest
state is `running` and its expiry is at or before the snapshot time. It is
`inconsistent` when a lease remains for a terminal operation. Absence of a
lease is rendered as no current operation. Status reports these facts but does
not take over or remove an expired or inconsistent lease.

The command does not expose an `available` or `reusable` boolean. Reusability
requires fresh Git and filesystem checks at an access boundary, so deriving it
from a possibly stale database snapshot would overstate what status knows.

### Read One Consistent Snapshot

Status captures one `snapshot_at` timestamp and loads workspaces, claims,
current operation leases and facts, latest operation states, and associated
repo worktrees inside one read-only SQLite transaction. The projection layer
uses `snapshot_at` for every lease-expiry comparison. It performs no write,
starts no workspace operation, and invokes no Git or filesystem observation.

Repository queries should batch related rows and assemble the projection by
workspace ID rather than issuing one query per relationship for each
workspace. Paths use lexicographical order, and repo worktrees within each
workspace are ordered by worktree path, making human and JSON output stable.

### Provide a Human Summary and Versioned JSON

Human output contains one row per workspace with these columns:

- `STATE`: persisted workspace health.
- `USAGE`: `claimed` or `unclaimed`.
- `OPERATION`: `none`, `active:<kind>`, `expired:<kind>`, or
  `inconsistent:<kind>`.
- `MODE`: `automatic` or `manual`.
- `REPOS`: total repo-worktree count followed by nonzero state counts.
- `RECONCILED`: the last reconciliation timestamp or `never`.
- `PATH`: canonical workspace path.

The renderer uses complete values without color-dependent meaning. Column
spacing is presentation detail rather than a parsing contract. When no rows
match, it prints `No workspaces.` and exits successfully.

`--json` emits exactly one JSON document on standard output with this envelope:

```json
{
  "schema_version": 1,
  "snapshot_at": "2026-09-08T12:00:00.000Z",
  "workspaces": []
}
```

Each workspace object contains `workspace_id`, `path`, `management_mode`,
`state`, `created_at`, `updated_at`, `last_reconciled_at`, `last_released_at`,
`reclaimed_at`, nullable `pool_id`, nullable `claim`, nullable
`current_operation`, and a `repo_worktrees` array. Claim objects contain
`claim_id` and `claimed_at`. Operation objects contain `operation_id`, `kind`,
`state`, `lease_id`, `lease_expires_at`, and `lease_status`. Repo-worktree
objects contain `repo_worktree_id`, `origin_repository_id`, `source_path`,
`worktree_path`, `state`, `last_head`, and `last_observed_at`.
Identifiers and timestamps are JSON strings, absent optional values are JSON
`null`, and the workspace array follows the same canonical-path order as human
output. Human diagnostics go to standard error and never contaminate JSON
standard output.

### Treat Reported Problems as Data

Degraded workspaces, dirty or missing worktrees, active claims, and expired or
inconsistent operation leases do not make the command fail. They are the state
the command was asked to report. Status returns a nonzero exit only when it
cannot load or serialize the snapshot.

## Risks / Trade-Offs

- [Persisted state may be stale after external Git changes] → Show
  `last_reconciled_at` and per-worktree observation times, and avoid an
  availability claim.
- [A full inventory can grow large] → Keep human output at workspace summary
  granularity, batch database reads, and reserve pagination or filters for a
  later change backed by measured need.
- [Versioned JSON can become a compatibility burden] → Add an explicit schema
  version and evolve it intentionally rather than treating table formatting as
  an API.
- [Leaked terminal leases reveal an otherwise hidden invariant violation] →
  Render them as `inconsistent` without attempting to repair them.

## Migration Plan

No data migration is required. Rolling back removes the command and its query
projection while leaving all lifecycle records unchanged.

## Open Questions

None.

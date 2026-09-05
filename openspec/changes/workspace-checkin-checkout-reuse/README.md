# Workspace Checkin and Checkout Reuse

This change defines a safe reuse lifecycle for Trees workspaces. An automatic
`create` request is keyed by repository directories, first reuses an idle
workspace from the matching pool, and provisions a new workspace under the
Trees-managed root only when no safe slot is available. The allocated
workspace is immediately held by a checkout lease and returned with
`checkin`. Automatically managed workspaces can later be reclaimed by an
explicit time-bounded `gc` command. Normal GC reports how many automatic
workspaces are currently not checked out and asks for confirmation. `--yes`
skips that confirmation while keeping normal safety checks; `--force` also
skips confirmation and explicitly enables destructive cleanup of otherwise
unsafe automatic slots.

```sh
trees gc --older-than 30d --dry-run
trees gc --older-than 30d
trees gc --older-than 30d --yes
trees gc --older-than 30d --force
```

The normal command reports automatic, not-checked-out, checked-out, and
reclaimable counts before confirmation. `--yes` bypasses confirmation without
relaxing safety checks. `--force` implies `--yes` and may delete local changes
in age-qualified automatic workspaces.

Workspaces have persisted `automatic` and `manual` management modes inferred
from the command shape. Automatic creation uses repository directories and no
concrete workspace path; manual creation requires a path. Only automatic
workspaces participate in pool allocation, checkin, and GC; manual workspaces
remain outside automated retention. Pool exhaustion provisions a new managed
slot, but no operation discards Git changes.

Automatic workspace paths are generated below a configured Trees
`workspaces_dir`; callers do not provide a concrete path. If it is not
configured, Trees uses the platform data-directory default. The lifecycle
database remains separate from this content directory, and persisted paths are
absolute.

Lease and operation records are updated in short SQLite transactions. Git and
filesystem work runs outside those transactions; renewal only extends an
active lease when the caller's work outlives the configured lease duration.

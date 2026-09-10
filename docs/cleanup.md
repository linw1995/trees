# Workspace and Source Cleanup

[Back to Trees](../README.md#documentation)

## Reclaim Old Automatic Workspaces

Reclaim old automatic workspaces with an explicit age threshold:

```sh
trees gc --older-than 30d --dry-run
trees gc --older-than 30d --yes
trees gc --older-than 30d --force
```

Normal GC reports automatic, unclaimed, claimed, age-eligible, and
candidate counts before asking for confirmation. `--yes` skips confirmation
while keeping normal safety checks. `--force` also skips confirmation and may
remove dirty worktrees or unexpected content, but never bypasses manual,
claim, operation, root-containment, or repository-identity guards.

## Remove Individual Workspaces

Remove one known workspace by the stable ID shown in
`trees status --view workspaces`:

```sh
trees remove <workspace-id> --dry-run
trees remove <workspace-id> --yes
trees remove <workspace-id> --force
```

Explicit removal accepts automatic and manual workspaces and does not apply an
age threshold. Normal mode requires a safe clean workspace. `--force` may
remove dirty worktrees or unexpected content, but it does not break an active
claim or operation and does not bypass path or repository identity guards.
Successful removal keeps the workspace and worktree records as reclaimed
tombstones.

## Remove a Source Record

The same remove command accepts a source repository ID from the repos view:

```sh
trees remove <repo-id> --dry-run
trees remove <repo-id> --yes
```

For repository targets, removal deletes only an origin record with no worktree
or pool references. Source files remain intact. The preflight reports reference
counts; retained history also counts, even after workspace removal. `--force`
skips confirmation but cannot bypass these guards or delete source files.
Removed IDs become unknown. Registering the surviving source again assigns a
new ID. Referenced records and their workspace history are preserved.

Use [Status and opening](status.md) to find workspace and source IDs.
See [Release automatic workspaces](workspaces.md#release-automatic-workspaces)
to return a claimed workspace to its pool.

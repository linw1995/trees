# Design

## Context

Status loads one read-only SQLite snapshot and closes the connection before
live observations. A multi-repository workspace root is a container that should
contain only the expected worktree paths and managed `AGENTS.md`. The existing
workspace-root validation already identifies unexpected entries, but status
does not report them.

## Goals / Non-Goals

Goals: report unexpected entries for the selected container workspace; keep
persisted state unchanged; keep observation failures nonfatal; leave the
workspace inventory and repository states on persisted data.

Non-goals: observe single-repository worktree roots, run Git, change repository
labels, reconcile persisted state, list file contents, or scan disk usage.

## Decisions

- Observe only the selected target and only when its root is a container. A
  single-repository workspace whose root is the worktree is skipped.
- Run the observation after the snapshot transaction closes.
- Reuse the workspace-root validation rules. Collect all unexpected entries,
  exclude managed `AGENTS.md`, sort paths, and limit the reported list while
  retaining the full count.
- Report missing and removed roots without reading entries. Map unreadable roots
  and instructions to stable issues.
- Add a target-only `target_workspace_observation` JSON field. Keep
  `schema_version` 2 because the change is additive.
- Do not add a CLI switch. The observation is a bounded directory read.

## Risks / Trade-Offs

- Reading the workspace root adds work proportional to its direct entry count,
  not to repository size.
- The entry list is bounded. The full count remains available and truncation is
  explicit.
- Filesystem errors are report data, not command failures.

## Migration Plan

No storage migration. Existing JSON consumers receive one additional nullable
field. Workspace inventories and repository detail tables keep their existing
shape.

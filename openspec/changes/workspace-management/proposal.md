## Why

Developers need one workspace directory that groups several independent Git repositories without copying or nesting the source repositories. The tool also needs durable lifecycle records so externally changed worktrees and failed operations can be understood later.

## What Changes

- Define the `trees create <workspace-path> --repo <repository-path>...` command contract.
- Define a workspace layout in which every repository worktree is a direct child of the workspace directory.
- Define repository-name child paths, collision handling, and detached worktrees from the current `HEAD`.
- Define cleanup of worktrees already created when a later repository operation fails.
- Define one global `db.sqlite` containing all workspace and repo-worktree lifecycle snapshots in the platform-standard home state directory, without per-workspace database isolation.
- Define append-only lifecycle events for normal transitions, external Git changes, failures, and rollback steps.
- Define reconciliation before and after relevant workspace operations without introducing a resident daemon.
- Define Clap as the CLI parser and synchronous Diesel with `diesel_migrations` as the SQLite database layer.
- Prohibit ad hoc raw SQL in the runtime database layer and follow the existing Coco dependency/toolchain organization where applicable.
- Keep user-facing status/history commands, non-default branch/ref policy, manifest format, and workspace deletion outside this change until separately discussed.

## Capabilities

### New Capabilities

- `workspace-management`: Create a workspace containing direct child Git worktrees for multiple repositories.
- `workspace-lifecycle`: Persist current lifecycle state and append-only events, including externally observed changes and failed operations.

### Modified Capabilities

None.

## Impact

- Adds the first reviewed product behavior contract for the Rust CLI.
- Requires a local SQLite state database outside the workspace directory.
- Adds synchronous `diesel`, `diesel_migrations`, and Clap implementation dependencies during the later implementation phase.
- Requires Git metadata reconciliation at workspace operation boundaries.
- Does not authorize implementation until this change is approved.

## Context

The Rust/Flake project foundation is complete, but no product behavior has been implemented. The workspace layout is already constrained to a root directory whose direct children are repository worktrees. This change adds the reviewed product model for creation and lifecycle tracking without deciding later CLI extensions.

## Goals / Non-Goals

**Goals:**

- Make direct child Git worktrees the canonical workspace layout.
- Persist a current snapshot and immutable lifecycle events in SQLite.
- Detect external Git worktree changes at operation boundaries.
- Preserve failure and rollback history for later diagnostics.
- Keep lifecycle data in platform-standard home state storage.
- Use Clap for CLI parsing and synchronous Diesel APIs for SQLite access.

**Non-Goals:**

- Implementing the CLI in this planning change.
- Adding user-facing status/history commands.
- Defining branch/ref selection, manifest export, workspace deletion, or background watching.
- Choosing a Rust SQLite crate or finalizing the complete state enum before implementation review.

## Decisions

### Direct Child Git Checkout Layout

The workspace root contains one directory per repository worktree, named from the source repository name. Name collisions fail before any worktree is created. This keeps the layout visible and predictable while preserving the source repositories and their Git administrative data outside the workspace.

### Use a Detached Initial Checkout

The initial command creates each direct child worktree from the source repository's current `HEAD` in detached mode. Branch and ref selection are intentionally deferred so the first command has one deterministic checkout rule.

### Clean up Partial Creation

Worktree creation is treated as a scoped operation. If a later repository fails, previously created worktrees from that operation are removed, while the lifecycle event log preserves the failure and cleanup narrative. Git remains authoritative for the cleanup result, which is reconciled on the next relevant operation.

### Snapshot and Event Log

The current snapshot supports reconciliation and recovery decisions without replaying the full history. The append-only event log preserves the operation narrative, including external observations and rollback details. Snapshot updates and corresponding events should be committed atomically.

### Use Four Persistence Entities

The SQLite model contains workspace snapshots, repo-worktree associations, operations, and immutable lifecycle events. The current snapshot is optimized for reconciliation; the event log is optimized for preserving the operation narrative.

### Define Relational Snapshot Tables

The proposed schema uses these logical columns:

- `workspaces`: `id`, `canonical_path`, `state`, `created_at`, `updated_at`, and `last_reconciled_at`.
- `repo_worktrees`: `id`, `workspace_id`, `repository_identity`, `source_path`, `worktree_path`, `state`, `last_head`, and `last_observed_at`.
- `operations`: `id`, `workspace_id`, `kind`, `state`, `owner_id`, `lease_expires_at`, `last_heartbeat_at`, `started_at`, `finished_at`, `pending_step`, `intent_json`, and `error_json`.
- `lifecycle_events`: the confirmed event identity/state fields plus `details_json` and `error_json`.

Foreign keys connect repo worktrees and operations to workspaces. A workspace path is unique; a workspace worktree path and repository identity are each unique within that workspace.

### Index State and Event Lookups

The implementation should index workspace path, workspace membership, repository identity, worktree path, operation time, and event time. Event ordering uses `occurred_at`; UUID v7 supplies time-local identifiers and index locality but does not replace the event timestamp.

### Use Stable Identities

Workspace, operation, and event records use UUIDs. Repository identity uses the canonical Git common directory, while worktree identity uses the canonical worktree path. Names and display paths are mutable metadata, not primary identity.

### Use an Explicit State Vocabulary

The initial state vocabulary is `creating`, `ready`, `degraded`, and `failed` for workspaces; `pending`, `attached`, `missing`, `diverged`, and `failed` for repo worktrees; and `running`, `succeeded`, `failed`, and `rolled_back` for operations. State changes append events with the previous and current values.

### Platform-Standard Home State Storage

Lifecycle data is not workspace content, so it belongs in the platform-standard application state location. The SQLite database is shared by workspace paths and is not placed in a repo worktree.

### Use One Global Database

All workspaces share one `db.sqlite` in the platform-standard `trees` state directory. Workspace isolation is logical through `workspace_id` relationships and constraints, not through separate database files.

### Store JSON as Text

SQLite has no native JSON column type. Structured details are serialized with the Rust JSON layer and stored as canonical JSON text in Diesel `Text` columns. JSON1 functions can be used by migration definitions or typed query expressions where needed, while runtime reads and writes remain Diesel query-builder operations.

### Generate Ordered Identifiers in Rust

SQLite has no native UUID type or UUID generator. The application generates UUID v7 values and stores their canonical text representation in `Text` columns. UUID v7 provides a time-ordered prefix for index locality; explicit event timestamps remain authoritative for chronology.

### Boundary Reconciliation Without a Daemon

The first lifecycle implementation reads the stored snapshot, queries Git's authoritative worktree metadata, compares stable identities and observed state, then writes any changes before and after relevant CLI operations. A resident watcher would add process ownership, shutdown, locking, and cross-platform filesystem monitoring concerns without being required for the initial contract.

### Explicit Operation and Event Identity

Every operation and event receives a stable identifier. Event source distinguishes CLI actions from external reconciliation, allowing a future consumer to avoid treating external changes as self-authored mutations.

### Persist Intent Before Git Mutations

Each Git mutation is preceded by a committed `running` operation record that describes the intended source repository, target worktree path, and pending step. A database failure before this commit prevents the Git mutation, which avoids untracked work caused by an operation that was never recorded.

### Recover Non-Terminal Operations

The next relevant invocation scans operations that remain `running` and compares their intended mutations with Git metadata. A complete match becomes `succeeded`; a partial or absent result becomes `failed` or `rolled_back` after cleanup. Recovery is represented by an event and does not require a new public status/history command.

### Use Clap for the CLI Boundary

The command surface SHALL use Clap's typed parser and derive-based command definitions. Argument validation and command dispatch should remain separate from workspace orchestration and persistence modules.

### Use Synchronous Diesel for SQLite

The database layer SHALL use synchronous `diesel` with the SQLite backend and `diesel_migrations` for embedded schema migrations. Connections use `SqliteConnection`; the implementation does not introduce an async runtime or `diesel-async`. The Flake development shell should expose `diesel-cli` consistently with the reference project.

### Avoid Raw Runtime Queries

Application and repository code SHALL use Diesel's schema declarations and query builder. It SHALL NOT call `diesel::sql_query`, `diesel::sql!`, `SimpleConnection::batch_execute`, or manually interpolated SQL for runtime reads or writes. Fixed PRAGMA statements required during SQLite connection initialization are the explicit exception.

### Configure SQLite Connections

Writable connections initialize foreign key enforcement, WAL journaling, and `busy_timeout = 5000`. The connection setup is the only runtime location allowed to issue these fixed SQLite configuration statements; no user input is interpolated into them.

### Serialize Operations Per Workspace

The schema enforces one non-terminal operation per workspace, while operations for different workspaces remain concurrent. Each operation carries an owner identity, lease expiry, and heartbeat timestamp so a later invocation can distinguish an active operation from an abandoned one.

### Keep Git Outside Database Transactions

The intent transaction is committed before Git starts. Git runs outside the database transaction. A separate short transaction commits the resulting snapshot, event, and operation step together. This keeps SQLite locks short and makes interruption recoverable through the persisted intent.

## Risks / Trade-Offs

- [External changes between reconciliation and mutation] → Reconcile at both operation boundaries and record observed state; implementation must still use Git's operation result as authoritative.
- [Home database loss or corruption] → Keep workspace directories independently usable as Git worktrees and make database failures explicit; recovery policy is a later implementation concern.
- [SQLite state and filesystem state diverge after interruption] → Record operation phases and reconcile on the next relevant invocation rather than assuming the last event represents the final filesystem state.
- [No query command in the first version] → Preserve a stable event/snapshot schema so future commands can consume the records without changing the core lifecycle model.

## Migration Plan

There is no existing lifecycle database or workspace format to migrate. A later implementation will initialize the SQLite schema lazily in the platform-standard state directory and reconcile existing workspaces before recording new transitions.

## Open Questions

The raw SQL prohibition applies to runtime database access, while `diesel_migrations` migration files remain allowed because they are the migration mechanism. The first migration fixes the relational columns, indexes, constraints, immutable event triggers, and JSON error encoding. The state names and core event fields are fixed by these requirements.

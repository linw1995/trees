## 1. Persistence and Domain Foundation

- [x] 1.1 Add synchronous Diesel SQLite dependencies and embedded `diesel_migrations`; verify `SqliteConnection` is used and the Flake exposes `diesel-cli`
- [x] 1.2 Resolve the platform-standard state directory and global `db.sqlite` path; verify the database is shared across workspaces and kept outside workspace content
- [x] 1.3 Define migrations for the four lifecycle entities, UUID v7 text identities, JSON text details, exact state vocabulary, immutable events, indexes, and constraints; verify both migration directions
- [x] 1.4 Define typed lifecycle states, stable identities, canonical path handling, UUID v7 conversion, and JSON validation; verify invalid values cannot reach persistence
- [x] 1.5 Configure writable SQLite connections with foreign keys, WAL, and a five-second busy timeout; verify read/write connection setup
- [x] 1.6 Enforce the no-runtime-raw-SQL rule with source checks or review tooling; verify application database access uses Diesel schema and query builder APIs

## 2. Typed Persistence Operations

- [ ] 2.1 Implement Diesel models and repository operations for workspace snapshots, repo-worktree snapshots, operations, and lifecycle events
- [ ] 2.2 Persist operation intent, owner identity, lease expiry, and heartbeat before any Git mutation; verify a failed intent transaction prevents Git access
- [ ] 2.3 Enforce one non-terminal operation per workspace while allowing operations for different workspaces to run concurrently
- [ ] 2.4 Keep Git processes outside SQLite transactions; verify snapshot, event, and operation-step updates commit atomically in short transactions
- [ ] 2.5 Record successful transitions, failures, and rollback steps; verify current snapshots and immutable events remain consistent

## 3. Git and CLI Primitives

- [ ] 3.1 Define the Clap command boundary and keep parsing errors separate from workspace orchestration; verify the reviewed command shape
- [ ] 3.2 Validate repository inputs, target safety, and source/worktree relationships before creating an operation
- [ ] 3.3 Derive direct child names from repository names and reject collisions before any worktree is created
- [ ] 3.4 Implement Git worktree inspection, detached checkout from current `HEAD`, and worktree removal operations; verify source repositories remain outside the workspace

## 4. Workspace Creation Workflow

- [ ] 4.1 Implement the reviewed `trees create <workspace-path> --repo <repository-path>...` orchestration using the typed CLI, persistence, and Git layers
- [ ] 4.2 Reconcile the workspace before and after the create operation, then persist `creating`, `pending`, and `running` states at the appropriate boundaries
- [ ] 4.3 Execute each Git mutation only after its intent is committed, and record the resulting snapshot, event, and operation step in a short transaction
- [ ] 4.4 Clean up worktrees created earlier when a later repository fails; record failure and rollback events and avoid reporting a partial workspace as successful
- [ ] 4.5 Finalize successful creation as `ready`, `attached`, and `succeeded`; verify multiple direct child worktrees are present

## 5. Reconciliation and Recovery

- [ ] 5.1 Reconcile Git's authoritative worktree metadata at relevant operation boundaries; verify externally removed or changed worktrees update snapshots and produce events
- [ ] 5.2 Make reconciliation idempotent; verify unchanged observations do not append duplicate events
- [ ] 5.3 Recover non-terminal operations after process interruption or lease expiry; verify complete, absent, and partial Git results converge to terminal operation states

## 6. Verification

- [ ] 6.1 Add unit and migration tests for paths, identities, states, JSON details, connection setup, constraints, and persistence operations
- [ ] 6.2 Add integration tests covering workspace layout, preserved source repositories, snapshots, event ordering, failures, steps taken to roll back, external changes, and recovery
- [ ] 6.3 Run the foundation `prek` checks and the complete Rust/SQLite test suite; verify no user-facing `status` or `history` command is introduced by this change

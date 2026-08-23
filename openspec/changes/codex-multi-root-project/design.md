## Context

See `proposal.md` for the motivation and user-facing scope. The current CLI has a typed Clap boundary and dispatches `create` directly to the workspace module. The workspace database already stores one managed `worktree_path` and its corresponding `source_path` for every repository association, ordering associations by worktree path. No Codex integration state exists yet.

The current Codex app-server exposes experimental SQLite-backed project APIs, including `project/create`, `project/read`, `project/update`, and `thread/start` with `projectId`. The protocol uses JSON-RPC over stdio and requires the client to opt into `experimentalApi`. The Codex CLI can resume a durable thread by identifier, while its Desktop launcher accepts only one path and does not expose a project-id or multi-root launch contract.

## Goals / Non-Goals

**Goals:**

- Make a Trees workspace the authoritative source of the roots synchronized into its Codex project.
- Reuse one project per workspace and Codex home while tolerating external project deletion.
- Start a durable project-bound thread with all managed worktree roots available to the Codex runtime.
- Keep the interactive Codex client in control of the terminal after setup completes.
- Make the app-server boundary deterministic, testable, and independent of Git mutations.

**Non-Goals:**

- Opening or selecting a project in Codex Desktop.
- Calling a hosted ChatGPT App directly from Trees.
- Adding or removing workspace repositories as part of `trees codex`.
- Changing Git worktrees, branches, commits, or repository content.
- Exposing arbitrary app-server JSON-RPC commands as a general-purpose Trees interface.
- Adding prompt, model, approval, or sandbox flags beyond the inherited Codex configuration.

## Decisions

### Use Managed Worktrees as Codex Roots

The launcher reads `repo_worktrees.worktree_path` as the Codex project roots. These are the directories that Trees created for the workspace and are the paths Codex is allowed to modify. `source_path` remains repository provenance and is not passed as a project root. The existing deterministic worktree-path ordering is reused for root ordering so repeated launches produce the same project shape.

Before reading roots, the launcher runs the existing workspace reconciliation against Git's authoritative worktree metadata. A workspace that becomes degraded, or any association that is no longer an attached worktree, stops the launch before any Codex project mutation. The command does not reconcile again after handoff because Codex owns the subsequent interactive session and file changes do not by themselves change the recorded Git attachment metadata.

### Use App-Server as the Project Identity Source

Do not duplicate the opaque Codex `project_id` in the Trees database. Derive a deterministic idempotency key from the Trees workspace identity and include a stable `treesWorkspaceId` metadata entry in `project/create`. Repeating `project/create` with the same key returns the existing project in the current Codex home, so the response supplies the project identifier needed by the remainder of the current invocation.

If the key refers to a project that was externally deleted, use paginated `project/list` as an exceptional recovery path and match the unique `treesWorkspaceId` metadata entry. If no project matches, create a replacement with a fresh key. If multiple projects match, fail rather than guessing. This keeps app-server project state authoritative and avoids a Trees migration plus a second cross-system association state machine.

### Synchronize the Complete Root List

The project root list is treated as a materialized view of the ready workspace. The launcher reads the current project, compares its ordered roots with the current worktree paths, and calls `project/update` with the complete list only when they differ. It does not send one update per root and does not preserve roots that are no longer present in the Trees workspace.

### Use a Short-Lived App-Server Setup Process

The launcher starts the configured Codex executable as `codex app-server --stdio` with piped standard input, standard output, and standard error. A small JSON-RPC client sends `initialize`, the `initialized` notification, project operations, and `thread/start`, while matching responses to request IDs when notifications are interleaved. Existing `serde_json` and standard process APIs are sufficient; no network transport or persistent daemon is required for this command.

The setup process is terminated only after the project and thread have been durably created. Protocol output is never forwarded to the user's terminal. Standard error is captured for actionable setup errors and bounded before inclusion in a returned error.

### Do Not Discover a Shared Daemon in the Initial Implementation

The initial command deliberately does not search for or attach to an already-running app-server. In stdio mode, the child process itself is the connection: there is no socket discovery, stale-socket recovery, or cross-platform WebSocket dependency. The child inherits the user's `CODEX_HOME`, so its `codexHome` response and SQLite project state remain in the same Codex state space used by the later `codex resume` invocation.

Codex also supports a managed daemon started by `codex app-server daemon start`. Its default control endpoint is `$CODEX_HOME/app-server-control/app-server-control.sock`, and `codex app-server proxy` exposes a raw WebSocket byte stream over that socket.

Reusing it would require Trees to implement the socket handshake, framing, reconnect behavior, and daemon/version compatibility checks. That transport is intentionally deferred to a separate change; it is not a transparent newline-delimited JSON stream.

### Codex Resume Handoff

After `thread/start` returns, Trees starts the same executable with `resume <thread-id>` and attaches the current terminal. The setup process is no longer needed because the project and thread are persisted in the shared Codex home. The launcher sets the process working directory to the first worktree root; the thread request also carries all worktree roots as runtime workspace roots.

The user’s normal Codex configuration remains authoritative for model, authentication, approval, and sandbox policy. Trees does not add bypass, danger-full-access, or automatic approval arguments. If a configured policy cannot authorize a root, Codex’s normal permission behavior remains in effect.

### Keep External Setup Separate from Git Lifecycle Mutation

`trees codex` performs workspace reconciliation reads/writes and external Codex RPC/process operations, but does not persist a local project association. It does not create a workspace operation that mutates Git, and it does not transition workspace or repo-worktree lifecycle states beyond reconciliation observations. A failed handoff leaves the synchronized project and thread available for retry, while no Git rollback is required.

### Isolate the Process Boundary for Tests

The Codex executable path is an optional command argument defaulting to `codex`. The app-server client and interactive handoff runner are separated behind testable interfaces so tests can use a fake newline-delimited JSON app-server and fake resume executable. Integration tests verify request ordering, project idempotency, root synchronization, thread parameters, and exit-status propagation without requiring a real Codex login.

## Risks / Trade-Offs

- [Codex app-server APIs are experimental and may change] → Fail on unsupported methods or malformed responses with the method and compatibility context; keep protocol types localized and cover the required message shapes with fixtures.
- [The external project may be deleted or edited outside Trees] → Reconcile the complete root list on every launch; use paginated ownership-metadata recovery only when the deterministic idempotency key is no longer usable, and fail on ambiguity.
- [The Codex executable may not be installed or may be a different version] → Validate process startup before the handoff, report the configured path and standard error, and allow an explicit executable override for controlled environments.
- [The setup process can be interrupted after external project creation] → Retry the deterministic idempotency key first; if recovery is needed, find the unique ownership metadata record before creating a replacement, and never delete an existing project from Trees.
- [Runtime roots may not be writable under the user’s policy] → Preserve Codex’s configured permission behavior and surface the normal permission request/error instead of silently broadening access.
- [Desktop UI cannot be opened at an app-server project id] → Keep Desktop navigation out of this change and document the terminal `codex resume` handoff as the supported launch target.

## Migration Plan

1. No Trees database migration is required. Existing workspace state remains unchanged.
2. Implement deterministic project idempotency and metadata recovery lazily on the first `trees codex` invocation.
3. If the change is rolled back, no Trees data migration is needed; Codex projects and threads already created in the user’s Codex home are intentionally not deleted, and Git worktrees are untouched.

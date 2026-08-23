## Why

Trees already persists the worktrees that make up a workspace, but users still have to configure and launch Codex manually for each repository. This change gives a managed workspace one repeatable entry point that materializes the managed worktree paths as a Codex multi-root project and opens an interactive Codex session associated with that project.

## What Changes

- Add a `trees codex <workspace-path>` subcommand with an optional Codex executable override for local installations and tests.
- Validate that the target is a managed, ready workspace and derive project roots from its persisted worktree paths in deterministic order.
- Use a deterministic app-server idempotency key and workspace ownership metadata so repeated launches reuse one Codex project without duplicating project identity in the Trees database.
- Synchronize the Codex project through the experimental app-server protocol, including project creation, root replacement, and paginated recovery after external deletion.
- Start a durable thread assigned to the synchronized project, expose all worktree roots as runtime workspace roots, and hand the thread to the interactive `codex resume` client.
- Provide a model-visible logical monorepo manifest that names every managed worktree root and explains that the repositories should be edited as one coordinated workspace.
- Preserve user-configured Codex approval and sandbox defaults; the launcher SHALL NOT enable bypass or full-access modes implicitly.
- Return clear errors for invalid workspace state, missing Codex binaries, unsupported app-server versions, failed project synchronization, and failed thread handoff without mutating Git worktrees.
- Add unit, integration, and process-boundary tests plus concise CLI documentation.
- Keep opening or navigating the Codex Desktop UI outside this change because the supported `codex app` command accepts a single path and the app-server protocol does not expose project-window navigation.

## Capabilities

### New Capabilities

- `codex-launch`: Launch an interactive Codex session from a managed Trees workspace through a synchronized multi-root Codex project.

### Modified Capabilities

None.

## Impact

- Extends the Clap command surface and command dispatch in `src/cli.rs` and `src/main.rs`.
- Adds a Codex app-server JSON-RPC client and child-process handoff layer using the existing JSON and process facilities.
- Does not add a Trees database migration or persist the opaque Codex project identifier locally; app-server project state remains authoritative.
- Reuses the existing workspace and repository persistence APIs; no Git worktree mutation is introduced.
- Requires a compatible `codex` executable on `PATH` by default, or at the explicitly supplied override path.

## Purpose

This capability connects a managed Trees workspace to an interactive Codex session by keeping a Codex project synchronized with the workspace's managed worktrees and assigning each launched thread to that project.

## ADDED Requirements

### Requirement: Launch Codex for a Managed Workspace

The CLI SHALL provide `trees codex <workspace-path>` and SHALL launch an interactive Codex session for the addressed workspace. The command SHALL resolve the workspace path using the same canonical path rules as other workspace operations and SHALL reconcile the workspace with Git metadata before deriving project roots.

#### Scenario: Launch from a Ready Workspace

- **WHEN** the user invokes `trees codex <workspace-path>` for a managed workspace whose state is `ready`
- **THEN** the command proceeds to project synchronization and Codex thread launch

#### Scenario: Reject an Unknown Workspace

- **WHEN** the supplied path is not a managed workspace
- **THEN** the command fails with an error identifying that the workspace is not managed and SHALL NOT start a Codex process

#### Scenario: Reject a Non-Ready Workspace

- **WHEN** the managed workspace state is not `ready`
- **THEN** the command fails with the observed workspace state and SHALL NOT start a Codex process or mutate Git worktrees

#### Scenario: Reject an Externally Broken Worktree

- **WHEN** reconciliation finds that a recorded worktree is missing or no longer attached to its source repository
- **THEN** the command fails with the reconciled workspace state and SHALL NOT create or update a Codex project

### Requirement: Derive Project Roots from Managed Worktrees

The command SHALL use the persisted `worktree_path` of every repository association as the Codex project roots. It SHALL preserve a deterministic order, SHALL use absolute canonical paths, and SHALL NOT use the original `source_path` values as project roots.

#### Scenario: Build Roots for a Multi-Repository Workspace

- **WHEN** a ready workspace contains two tracked repository associations
- **THEN** the Codex project receives both managed worktree paths as ordered roots

#### Scenario: Reject an Incomplete Workspace Record

- **WHEN** the workspace has no tracked repository associations or an association has an invalid worktree path
- **THEN** the command fails before creating or updating a Codex project

### Requirement: Reuse and Synchronize the Codex Project

The command SHALL use a deterministic idempotency key derived from the Trees workspace identity and SHALL include workspace ownership metadata when creating a Codex project. The app-server response SHALL be the authoritative project reference for the current invocation. On every launch the command SHALL replace the Codex project's roots with the complete ordered workspace worktree path list when they differ, without requiring Trees to persist the opaque project identifier.

#### Scenario: Create or Reuse the Codex Project

- **WHEN** the command calls `project/create` with the workspace's deterministic idempotency key
- **THEN** app-server returns either a newly created project or the previously created project with all current worktree roots represented in the request

#### Scenario: Retry an Ambiguous Create Result

- **WHEN** project creation may have committed but the response is lost
- **THEN** the command retries with the same idempotency key before attempting any new project creation

#### Scenario: Synchronize Changed Roots

- **WHEN** `project/create` returns a project whose roots differ from the workspace worktree paths
- **THEN** the command updates the project with the complete current root list and preserves the root order

#### Scenario: Recover from External Project Deletion

- **WHEN** the deterministic idempotency key is reserved for a project that was externally deleted
- **THEN** the command paginates `project/list` and adopts the unique project carrying the workspace ownership metadata, or creates a replacement with a fresh idempotency key when no such project exists

#### Scenario: Reject Ambiguous Project Recovery

- **WHEN** project recovery finds more than one project carrying the same workspace ownership metadata
- **THEN** the command fails with an ambiguity error and does not update any project roots

### Requirement: Start a Project-Bound Codex Thread

After project synchronization succeeds, the command SHALL start a durable Codex thread assigned to the project. The thread SHALL use the workspace container as its working directory and SHALL receive all managed worktree roots as runtime workspace roots. The command SHALL preserve the user's configured approval, sandbox, model, and authentication settings unless explicitly overridden by a future command option.

#### Scenario: Start a Thread for All Roots

- **WHEN** project synchronization returns a valid project identifier
- **THEN** the command starts a thread with that `projectId`, the workspace container as `cwd`, and every worktree root in the runtime workspace root list

### Requirement: Expose the Logical Monorepo Context

Before starting the thread, the command SHALL obtain the effective user developer instructions and append a generated workspace manifest containing the workspace name and every ordered managed worktree root. The manifest SHALL state that the roots are independent repositories in one coordinated logical monorepo. The command SHALL preserve the existing developer instructions and SHALL NOT create or modify workspace instruction files.

#### Scenario: Make All Repositories Visible to the Model

- **WHEN** a ready workspace contains multiple managed worktree roots
- **THEN** the `thread/start` request contains model-visible context naming every root and describing the workspace as one coordinated multi-repository workspace

#### Scenario: Preserve User Developer Instructions

- **WHEN** the effective Codex configuration contains developer instructions
- **THEN** the generated workspace context is appended after those instructions rather than replacing them

#### Scenario: Stop Before Thread Launch on Project Failure

- **WHEN** project creation or root synchronization fails
- **THEN** the command reports the app-server error and SHALL NOT start the interactive Codex client

### Requirement: Hand off to the Interactive Codex Client

The command SHALL hand the newly started thread to the configured Codex
executable by invoking its interactive resume command with the returned thread
identifier. It SHALL pass the workspace container as the resume command's
`--cd` value and SHALL pass every managed worktree root as an `--add-dir`
value. The resume client opens a new app-server connection and does not inherit
runtime roots from `thread/start`. By default, the executable SHALL be resolved
as `codex` from `PATH`; an explicit executable override SHALL be supported. The
command SHALL return the Codex client's exit status.

#### Scenario: Resume the Created Thread

- **WHEN** app-server returns a durable thread identifier
- **THEN** the command invokes the Codex client's resume operation for that identifier with the workspace container as `--cd` and every worktree root as `--add-dir`
  while keeping the user's terminal attached to the interactive session

#### Scenario: Codex Executable Is Missing

- **WHEN** the default or explicitly configured Codex executable cannot be started
- **THEN** the command fails with the executable path and operating-system error, and SHALL leave Git worktrees unchanged

#### Scenario: Codex Client Exits

- **WHEN** the interactive Codex client exits
- **THEN** the command terminates and returns the client's success or failure status to the caller

### Requirement: Keep the Launch Operation Safe

The command SHALL communicate with app-server through the experimental protocol handshake and a bounded JSON-RPC session. It SHALL match responses by request identifier while tolerating interleaved notifications.

Malformed messages, EOF, process failure, and request timeout SHALL fail setup. The interactive client SHALL start only after project and thread state are durable and the setup app-server process has exited.

The command SHALL NOT enable approval bypass or unrestricted sandbox flags implicitly. A failed launch SHALL leave any synchronized Codex project and thread available for retry but SHALL NOT modify Git worktree state.

#### Scenario: Experimental Protocol Is Not Available

- **WHEN** app-server rejects the experimental capability or does not support a required project or thread method
- **THEN** the command fails with an actionable protocol compatibility error and SHALL NOT invoke the interactive Codex client

#### Scenario: Interleaved Notifications During Setup

- **WHEN** app-server emits lifecycle or warning notifications between responses to requests issued during setup
- **THEN** the command continues matching the expected responses by request identifier and does not mistake a notification for a failed response

#### Scenario: Transport Failure During Setup

- **WHEN** a setup request times out, standard output contains malformed JSON, app-server exits early, or standard input cannot be closed cleanly
- **THEN** the command reports a bounded setup error, reaps or terminates the child process, and SHALL NOT invoke the interactive Codex client

#### Scenario: Handoff After Confirmed Setup Exit

- **WHEN** project and thread creation succeed and the setup app-server process exits successfully after standard input is closed
- **THEN** the command immediately invokes `codex resume <threadId>` without waiting for a `thread/closed` notification or an idle-unload interval

#### Scenario: Retry After Client Handoff Failure

- **WHEN** project and thread creation succeed but the interactive client cannot be launched
- **THEN** the command reports the handoff failure while retaining the project and thread in app-server state for a later retry

#### Scenario: Preserve User Security Defaults

- **WHEN** the command launches app-server and the interactive client
- **THEN** it inherits the user's configured security and authentication settings and does not add bypass or danger-full-access arguments

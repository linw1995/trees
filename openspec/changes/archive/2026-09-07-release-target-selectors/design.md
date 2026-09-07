## Context

See `proposal.md` for motivation. The active claim table already provides a global claim identifier and a unique workspace foreign key, while current operation leases provide a unique workspace foreign key for operation exclusion. Release currently requires both identities and uses the retrying operation-admission path.

## Goals / Non-Goals

**Goals:**

- Represent the CLI input as an optional workspace directory or claim identifier, with a current-directory default.
- Resolve every target to one workspace and the active claim observed for that invocation.
- Make concurrent release admission fail immediately instead of waiting for another writer or retrying after the active operation completes.
- Preserve the existing reconciliation and atomic release behavior after admission.

**Non-Goals:**

- Change the claim or operation-lease schema.
- Add claim expiry, ownership, heartbeat, or automatic abandonment.
- Make release idempotent after the selected claim has already been removed.
- Change automatic allocation or garbage-collection admission behavior.

## Decisions

### Use an Optional Release Target

The CLI accepts a positional workspace directory or `--claim-id`, enforced as one optional, mutually exclusive argument group. When neither input is present, release selects the process current directory. This keeps each explicit invocation's intent unambiguous and intentionally rejects the previous redundant path-plus-claim form.

An explicit directory may be absolute or relative and resolves only an exact managed workspace. A relative directory is resolved against the process current directory. The no-argument form canonicalizes the process current directory and walks its ancestors, selecting the nearest managed workspace so the command works from a repository subdirectory. A claim identifier resolves through the active claim row and then its workspace.

### Snapshot the Active Claim Before Operation Admission

Path and current-directory targets read the current claim identifier before they attempt release admission. The resolved path and claim identifier then use the existing exact release workflow. A claim change between resolution and admission therefore causes the release to fail instead of deleting a later claim.

The alternative of resolving the current claim after admission would make a delayed path invocation operate on a newer claim than the one it initially observed.

### Add Release-Specific Try-Once Admission

Release uses one immediate transaction with SQLite's busy timeout temporarily set to zero. A workspace operation uniqueness conflict or SQLite busy result maps to the existing workspace-busy error. The normal connection timeout is restored before returning.

General operation admission keeps bounded retries because create, acquire, and garbage collection have different retry behavior. Reusing it for release would allow a concurrent request to wait and run after the first release finishes.

## Risks / Trade-Offs

- [Path and current-directory targets release the currently observed claim rather than proving caller identity] → Keep `--claim-id` for automation that needs exact acquire-to-release correlation and document the distinction.
- [A global SQLite writer unrelated to the target workspace can cause fail-fast release admission] → Report the same busy outcome and require an explicit caller retry; this preserves try-or-exit semantics.
- [Temporarily changing the connection busy timeout could leak into later operations] → Restore the configured timeout on every non-panicking result path and cover lock contention with a regression test.
- [The old combined CLI form breaks existing scripts] → Mark the change as breaking and document the workspace-directory, no-argument, and claim-identifier forms.

## Migration Plan

Update callers to pass a workspace directory, pass `--claim-id`, or rely on the no-argument current-directory default. Rolling back restores the previous parser and retrying admission path; no database migration is required.

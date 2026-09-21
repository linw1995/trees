# Design

## Context

See `proposal.md` for motivation. `acquire_automatic_candidate` aligns worktrees before granting a claim, so it cannot implement in-place claiming. Reconciliation classifies dirty files as `dirty` and branch or revision changes as `diverged`, making workspace health `degraded`. These observations do not necessarily indicate structural damage.

The project already has typed workspace selection, unique active claims, operation leases, immutable events, and short transaction helpers. `record_workspace_acquire` currently inserts the claim and access event separately from terminal operation completion; direct claiming needs a stronger atomic publication boundary.

## Goals / Non-Goals

**Goals:** Preserve user work, share existing identity and lease protocols, and provide deterministic target selection and auditable outcomes.

**Non-Goals:** Pool allocation, manual-to-automatic conversion, claim takeover, process ownership inference, opening programs, automatic release, Git alignment, and changes to existing command options.

## Decisions

### 1. Use a Command-Specific Argument Type with Shared Lookup

Add `ClaimArgs` with an optional positional directory, `--workspace-id`, and `--json`. Convert into existing `WorkspaceSelector` variants and call the shared locator. Validate conflicts for both CLI parsing and directly constructed arguments before filesystem or database access. Reuse conversion helpers where practical without flattening `WorkspaceLocatorArgs`, which would expose the rejected named directory and claim-ID options. Other commands retain their interfaces.

### 2. Validate Workspace Structure

Add the orchestration entry point in the existing workspace module. Require automatic mode, an existing pool with matching repository membership, a present registered workspace root, and a nonempty complete set of managed worktrees. Verify source common-directory identities, expected canonical worktree paths, Git registration, worktree identities, and non-prunable status.

Reject removed, creating, failed, incomplete, or structurally damaged workspaces and unresolved addition/removal state. A `degraded` snapshot alone is not a rejection: fresh observations must distinguish dirty content and branch/revision drift from identity or membership damage. Do not reuse the `ready` predicate or accept all `diverged` observations blindly.

Permit staged, unstaged, untracked, and ignored content, attached branches, and changed `HEAD` values when structural validation passes. Reconcile observed health under the operation lease without relabeling the workspace as ready merely because it is claimed. Preserve existing reconciliation bookkeeping semantics; do not rewrite Git state or introduce a new reusable baseline to make claim admission pass.

### 3. Use a Dedicated Claim Operation and Atomic Publication

Resolve the target, then acquire existing per-workspace operation admission with operation kind `claim`. Recheck mode, lifecycle eligibility, pool membership, and claim absence under admission. Inspect Git outside database transactions, maintaining the lease during inspection. Perform final structural observation before publication.

In one short transaction, verify the current unexpired lease and persisted admission conditions,
insert a fresh `WorkspaceClaim`, append `workspace_claimed`, mark the operation succeeded, append
its terminal event, and remove its lease. Preserve workspace identity, membership, management mode,
pool, and last release timestamp. Reuse existing storage helpers within the outer transaction where
they preserve this boundary; add a focused finalization helper if needed. Do not change the existing
automatic acquisition workflow incidentally.

Unique claim constraints and operation admission serialize claim versus claim, create, GC, release, add, and removal. A lost race fails for the selected target; it never switches targets or allocates a replacement.

### 4. Fail Conservatively Around Retained Operations

Claim refuses any retained operation lease, including expired leases, and unresolved mutation journals. It does not initiate recovery of another command: recovery could move or remove worktrees, violating the in-place contract. This is an explicit command-specific exception to automatic recovery before mutation. Report the blocking operation and direct users to its existing recovery workflow.

Before publication, an error terminates the owned operation as failed where persistence is available and creates no claim. If a process dies first, its retained lease blocks new claims; existing recovery handles an expired claim operation as a non-creation operation without deleting worktrees or manufacturing a claim. A claim is never committed with a running operation because publication is atomic.

If commit succeeds but output is lost, the claim remains active. Retrying reports an existing claim; users inspect status and use ordinary release. Do not auto-release on output failure or return an existing claim as if the retry acquired it.

### 5. Return a Dedicated Result Without Changing Create Output

Return `workspace_id`, `workspace_path`, `pool_id`, and `claim_id`. Default output uses Bash-safe assignments `WORKSPACE_ID`, `WORKSPACE_PATH`, `POOL_ID`, and `CLAIM_ID`; JSON uses the lowercase field names. Reuse quoting helpers, keep diagnostics on standard error, and emit success only after commit.

## Risks / Trade-Offs

- A healthy structure can still have `degraded` health from user edits. Mitigation: test health and claim as independent dimensions; keep automatic reuse checks unchanged.
- Git or filesystem edits outside Trees do not honor leases. Mitigation: validate near publication and document that a claim coordinates Trees operations, not external filesystem writers.
- Conservative admission requires recovery through another command after interruption. Mitigation: identify the retained operation and cover expired claim recovery explicitly; never silently run destructive recovery from claim.
- Claims do not override explicitly forced removal. Mitigation: preserve the existing removal policy and avoid promising filesystem locking.
- Existing release can reject dirty work and align clean worktrees. Mitigation: document that claim preserves content while release retains its existing safety and alignment behavior.

## Migration Plan

No schema migration is expected: operation kinds are stored as text and active claims already have the necessary fields and uniqueness. Add code, tests, and documentation together. A rollback to the previous binary still recognizes and releases committed claims; confirm that its generic non-creation recovery handles interrupted claim operations. Do not modify unrelated existing specification drift, including release alignment wording, in this change.

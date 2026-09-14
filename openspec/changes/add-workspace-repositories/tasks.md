## 1. CLI and Target Contract

- [x] 1.1 Add `AddArgs` using mainline `WorkspaceLocatorArgs`, its shared exclusion group and current-directory default, optional positional directory, repeatable repo inputs, offline and JSON flags.
- [x] 1.2 Use existing typed selectors and `locate` in a caller-owned snapshot; preserve selected-claim provenance, capture active claims, and keep recovery/admission outside the locator.
- [x] 1.3 Define typed `add` request, execution plan, result, and Snafu error types in a dedicated `add` module.

## 2. Operation Persistence and Admission

- [x] 2.1 Add one-shot lease admission with atomic claim validation and immutable `kind = add` request intent; reject idle automatic workspaces.
- [x] 2.2 Define versioned resolved-plan and step payloads, event details, no-op/rejection auditing, and lease renewal through source provisioning and external work.
- [x] 2.3 Add a short finalization transaction for associations, relocated paths, exact pool lookup/creation, observed health, terminal events, and lease removal.
- [x] 2.4 Add residual/compensation persistence and consistent active-membership filtering so failed or removed associations cannot produce false no-op success or pool matches.

## 3. Planning and Directory Structure

- [x] 3.1 Reuse origin resolution and separate identity lookup from revision fetching; deduplicate `add` inputs and skip fetch for existing identities.
- [x] 3.2 Validate existing associations while allowing dirty, branch-attached, or changed-HEAD worktrees; reject broken identities and unresolved additions.
- [x] 3.3 Plan final child names, collisions, containment, symlinks, nested workspaces, and operation-owned staging before workspace mutation.
- [x] 3.4 Add lease-aware Git move and filesystem compensation helpers for root promotion, preserving worktree IDs and local content without force.

## 4. Execution and Recovery

- [x] 4.1 Implement recorded promotion and new worktree creation, final physical validation, and atomic publication; retain source provisioning diagnostics.
- [x] 4.2 Implement recorded reverse compensation, safe ownership checks, restoration of the original root, and terminal compensation/failure reporting.
- [x] 4.3 Amend the non-create recovery specification exception and `add` explicit addition recovery dispatch before generic non-create recovery; reconstruct the plan and resume publication or compensation after lease takeover.
- [x] 4.4 Handle unsafe residuals using a new recovery operation linked to a terminal failed `add`, and temporarily absent roots using existing shared selectors; prevent inconsistent `add`/release/reuse while preserving existing removal safety rules.

## 5. Output and Consumers

- [x] 5.1 Implement text and JSON schema version 1 results, unique repository outcomes, old/new paths and pool IDs, with diagnostics isolated to standard error.
- [x] 5.2 Verify status, release, reuse, removal, open, and project preparation consume committed expanded membership and relocated paths without resetting existing work during `add`.
- [x] 5.3 Document command examples, offline/idempotency rules, automatic claim requirements, pool changes, root promotion, running-tool limitations, retained clones, and repair/retry behavior.

## 6. Validation

- [ ] 6.1 Cover manual and claimed automatic additions. Check current directory/positional/named ID/path/claim selection; pairwise and direct-construction conflicts; explicit misses without fallback; nested removed boundaries; ID/claim lookup without current directory; stale claims; idle targets; input aliases; URL/name ambiguity. Check offline and output contracts.
- [ ] 6.2 Cover single-to-multiple promotion and existing child directory structures with staged, unstaged, untracked, ignored, branch-attached, and changed-HEAD content; verify preserved IDs and bytes.
- [ ] 6.3 Cover occupied/symlink paths, name collisions, broken identities, locked or unsupported worktree moves, and no-op auditing without fetch or directory structure changes.
- [ ] 6.4 Cover exact pool migration, existing destination pools, other-slot preservation, failed migration, unchanged claims, and release followed by exact-set reuse.
- [ ] 6.5 Inject failure before/after each journal, Git move/`add`, container creation, compensation, and final publication boundary; verify events, lease cleanup, atomicity, and safe retry.
- [ ] 6.6 Exercise recovery with a missing root, complete/partial directory structures, compensation already selected, dirty or ambiguous residuals, lost lease, and competing takeover; verify no user-content deletion.
- [ ] 6.7 Run focused integration tests and repository-required formatting, lint, and test checks; validate the OpenSpec change before implementation review.

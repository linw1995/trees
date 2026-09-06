## 1. Contract and Domain Model

- [x] 1.1 Define automatic `trees create --repo ...` allocation, manual
  `trees create <workspace-path> --repo ...`, claim acquisition, release
  command arguments, result output, error cases, and the UUID v7 claim
  identifier
- [x] 1.2 Define `automatic` and `manual` workspace modes inferred from
  positional-path presence, and the rule that manual workspaces bypass all
  automated acquire/release and GC behavior
- [x] 1.3 Define the UUID-backed repository-set pool registry and non-unique
  hash index. Define exact sorted origin repository ID matching,
  least-recently-used candidate ordering, configurable platform-specific
  `workspaces_dir`, absolute path normalization, generated path format,
  and allocation retry behavior after a claim race
- [x] 1.4 Define a typed workspace claim with an acquisition timestamp; keep
  expiry and renewal on operation leases only, and define
  explicit mode/access/health separation
- [x] 1.5 Extend lifecycle states with `dirty` and `reclaimed`, plus
  `last_released_at`, pool ID, and reclamation timestamps; update claim parsing,
  serialization, workspace-state
  aggregation, and affected validation paths

## 2. Persistence and Migration

- [ ] 2.1 Rework migration `00000000000002` so it builds the final workspace
  reuse schema directly from the original lifecycle tables. Preserve workspace
  and repo-worktree IDs and observations, keep `operations` append-only, add
  `operation_leases` for current lease state, move operation transitions to
  lifecycle events, and keep one active lease per workspace operation.
- [ ] 2.2 Backfill legacy explicit-path workspace records as `manual` without
  touching Git or the filesystem, migrate operation lease state into the
  separate `operation_leases` table, and provide a reversible down migration
  for workspace claims and operation leases
- [x] 2.3 Add Diesel schema/models and repository operations for origin
  repositories, pool relations, and acquiring, reading, and releasing claims
  without runtime raw SQL
- [ ] 2.4 Add typed persistence helpers for append-only operation facts,
  current `operation_leases`, acquire/release/GC events, lease renewals, lease
  takeover, and atomic terminal-event plus lease cleanup
- [ ] 2.5 Verify claim acquisition races, wrong-token rejection, mode,
  timestamp persistence, append-only operation facts, lease renewal and
  takeover races, tombstone retention, JSON detail validation, and migration
  upgrade/downgrade

## 3. Git Observation and Reconciliation

- [x] 3.1 Add a read-only Git worktree cleanliness probe using porcelain status
  and include staged, unstaged, and untracked changes in the observation
  fingerprint
- [x] 3.2 Map dirty observations to `dirty`, preserve `missing`/`diverged`/
  `failed` precedence for association failures, and make workspace degradation
  and recovery idempotent
- [ ] 3.3 Reconcile active claims at acquire and release boundaries while
  keeping Git and filesystem commands outside short database transactions;
  renew `operation_leases` while external steps run, append operation state
  transitions without mutating `operations`, and verify unchanged
  observations do not append duplicate events
- [x] 3.4 Add a non-forced worktree removal primitive and a workspace-root
  safety check that refuses to remove unexpected files or directories

## 4. Pool Allocation and Claim Workflow

- [x] 4.1 Implement automatic repository-set allocation that searches exact
  pool ID matches, filters reusable idle candidates, selects least-recently
  released workspaces, and retries after an acquisition race
- [ ] 4.2 Append allocation intent, create the current operation lease, and
  acquire the workspace claim atomically, then run final reconciliation and
  append a terminal event if the post-check fails
- [ ] 4.3 Implement automatic provisioning below the managed workspace root
  when no safe candidate exists, including an append-only creation intent,
  immediate operation lease and claim creation, and partial-creation rollback
- [ ] 4.4 Implement `operation_leases` renewals during long external steps with
  lease-token validation, short transactions, and recovery-safe expiry
- [ ] 4.5 Implement expired-operation recovery with a safe reconciliation gate
  and an atomic operation ID, lease-token, and expiry transition

## 5. Acquire and Release Workflow with GC Integration

- [x] 5.1 Implement token-protected release that retains the claim on dirty,
  missing, prunable, diverged, or failed worktrees and releases it only after
  a successful reusable-state check
- [x] 5.2 Wire Clap parsing and `main` dispatch for automatic create and
  release; print stable shell variables for workspace, pool ID, and claim ID
  while keeping human-readable errors
- [x] 5.3 Add `trees config set workspaces-dir <path>` and configuration
  loading, resolving configured paths to absolute values before persistence;
  keep database state and workspace content directories separate
- [x] 5.4 Implement `trees gc --older-than <duration> [--dry-run] [--yes]
  [--force]`
  with UTC cutoff calculation, `last_released_at`/`created_at` idle
  selection, automatic-mode-only candidate filtering using each slot path's
  derived parent root, and counts for unclaimed and claimed workspaces
- [ ] 5.5 Execute GC as one serialized operation per candidate using the
  current `operation_leases` row. Safely remove clean worktrees and the empty
  workspace root in normal mode; in `--force` mode allow explicitly authorized
  unsafe automatic-slot cleanup while retaining age, root, claim, operation,
  and repository-identity guards
- [x] 5.6 Add normal interactive confirmation, refusal without interaction or
  `--yes` or `--force`, safe `--yes` bypass, forced-run warnings, and stable
  candidate/skipped/reclaimed/failed/unclaimed/claimed counts; ensure
  dry-run performs no SQLite, Git, or filesystem write
- [x] 5.7 Keep manual `trees create` and existing `trees codex` flows
  compatible; document repository-set allocation, generated automatic paths,
  automatic/manual mode, claim lifecycle, and that only GC may remove an idle
  automatic workspace

## 6. Verification and Documentation

- [ ] 6.1 Add unit tests for claim state transitions, token authorization,
  UUID/timestamp serialization, append-only operation facts, current lease
  renewal/takeover, reusable predicates, and dirty-state reconciliation
- [ ] 6.2 Add integration tests for pool allocation, root resolution, generated
  paths, concurrency, operation lease recovery, rejected dirty release, and
  mode isolation. Cover dry-run GC, thresholds, safe and forced reclamation,
  confirmation, partial failures, external worktree removal, and Git identity
  preservation.
- [ ] 6.3 Run `openspec validate workspace-checkin-checkout-reuse --strict`,
  the complete Rust/SQLite test suite, `prek -a`, and
  `nix flake check --no-build` after the append-only operation and separate
  lease implementation is complete; distinguish spec validation from
  behavioral verification in the final evidence

## 1. Contract and Domain Model

- [x] 1.1 Define automatic `trees create --repo ...` allocation, manual
  `trees create <workspace-path> --repo ...`, lease renewal, and checkin
  command arguments, result output, error cases, and the UUID v7 checkout
  identifier
- [x] 1.2 Define `automatic` and `manual` workspace modes inferred from
  positional-path presence, and the rule that manual workspaces bypass all
  automated checkout/checkin and GC behavior
- [x] 1.3 Define the canonical repository-set pool key, least-recently-used
  candidate ordering, configurable platform-specific `workspaces_dir`,
  absolute root normalization, generated path format, and allocation retry
  behavior after a lease race
- [x] 1.4 Add a typed checkout lease model with owner, acquisition, expiry,
  and heartbeat timestamps; define the reusable-workspace predicate and
  explicit mode/access/health separation
- [x] 1.5 Extend lifecycle states with `dirty` and `reclaimed`, plus
  `last_checked_in_at`, pool-key, absolute workspace-root, and reclamation
  timestamps; update parsing, serialization, workspace-state aggregation, and
  affected validation paths

## 2. Persistence and Migration

- [ ] 2.1 Add migration `00000000000002` for management mode, pool key,
  absolute workspace-root namespace, idle and reclamation timestamps, the
  `dirty`/`reclaimed` state constraints, and the current `workspace_leases`
  table with one active lease per workspace
- [ ] 2.2 Backfill legacy explicit-path workspace records as `manual` without
  touching Git or the filesystem, and provide a reversible down migration
- [ ] 2.3 Add Diesel schema/models and repository operations for acquiring,
  renewing, reading, and releasing leases without runtime raw SQL
- [ ] 2.4 Add typed persistence helpers for checkout, checkin, GC operations,
  access/reclamation events, old/new lease details, and atomic operation
  completion
- [ ] 2.5 Verify lease acquisition races, wrong-token rejection, mode and
  timestamp persistence, tombstone retention, JSON detail validation, and
  migration upgrade/downgrade

## 3. Git Observation and Reconciliation

- [ ] 3.1 Add a read-only Git worktree cleanliness probe using porcelain status
  and include staged, unstaged, and untracked changes in the observation
  fingerprint
- [ ] 3.2 Map dirty observations to `dirty`, preserve `missing`/`diverged`/
  `failed` precedence for association failures, and make workspace degradation
  and recovery idempotent
- [ ] 3.3 Reconcile active leases at checkout and checkin boundaries while
  keeping Git commands outside database transactions; verify unchanged
  observations do not append duplicate events
- [ ] 3.4 Add a non-forced worktree removal primitive and a workspace-root
  safety check that refuses to remove unexpected files or directories

## 4. Pool Allocation and Renewal Workflow

- [ ] 4.1 Implement automatic repository-set allocation that searches exact
  pool-key matches, filters reusable idle candidates, selects least-recently
  checked-in workspaces, and retries after an acquisition race
- [ ] 4.2 Persist allocation intent and acquire the lease atomically, then run
  final reconciliation and release the lease with a failure event if the
  post-check fails
- [ ] 4.3 Implement automatic provisioning below the managed workspace root
  when no safe candidate exists, including generated paths, creation intent,
  immediate lease ownership, and partial-creation rollback
- [ ] 4.4 Implement identifier-based renewal with the fixed 24-hour extension,
  repository-set validation, lease heartbeat updates, and immutable renewal
  events
- [ ] 4.5 Implement expired-lease recovery with a safe reconciliation gate and
  an atomic old-lease removal/new-lease acquisition path

## 5. Checkin Workflow and GC Integration

- [ ] 5.1 Implement token-protected checkin that retains the lease on dirty,
  missing, prunable, diverged, or failed worktrees and releases it only after
  a successful reusable-state check
- [ ] 5.2 Wire Clap parsing and `main` dispatch for automatic create,
  renewal, and checkin; print stable machine-copiable workspace, pool key,
  checkout ID, and expiry fields while keeping human-readable errors
- [ ] 5.3 Add `trees config set workspaces-dir <path>` and configuration
  loading, resolving configured paths to absolute values before persistence;
  keep database state and workspace content directories separate
- [ ] 5.4 Implement `trees gc --older-than <duration> [--dry-run] [--yes]
  [--force]`
  with UTC cutoff calculation, `last_checked_in_at`/`created_at` idle
  selection, automatic-mode-only candidate filtering scoped to the resolved
  workspace root, and counts for not-checked-out and checked-out workspaces
- [ ] 5.5 Execute GC as one serialized operation per candidate, safely remove
  clean worktrees and the empty workspace root in normal mode; in `--force`
  mode allow explicitly authorized unsafe automatic-slot cleanup while
  retaining age, root, lease, operation, and repository-identity guards
- [ ] 5.6 Add normal interactive confirmation, non-interactive refusal without
  `--yes` or `--force`, safe `--yes` bypass, forced-run warnings, and stable
  candidate/skipped/reclaimed/failed/not-checked-out/checked-out counts; ensure
  dry-run performs no SQLite, Git, or filesystem write
- [ ] 5.7 Keep manual `trees create` and existing `trees codex` flows
  compatible; document repository-set allocation, generated automatic paths,
  automatic/manual mode, checkin ownership, and that only GC may remove an
  idle automatic workspace

## 6. Verification and Documentation

- [ ] 6.1 Add unit tests for lease state transitions, expiry, token
  authorization, UUID/timestamp serialization, reusable predicates, and
  dirty-state reconciliation
- [ ] 6.2 Add integration tests for repeated pool allocation/checkin,
  repository-set matching, least-recently-used selection, pool races,
  configured-root resolution, absolute path persistence, generated-path
  provisioning, concurrent allocation, renewal, stale-lease
  reclaim, post-allocation race detection, rejected dirty checkin,
  automatic/manual mode isolation, dry-run GC, threshold boundaries, safe
  reclamation, confirmation behavior, `--yes` safe bypass, force cleanup of
  dirty/diverged and extra-content workspaces, force protection boundaries,
  partial GC failure,
  external worktree removal, and preservation of all Git files and identities
- [ ] 6.3 Run `openspec validate workspace-checkin-checkout-reuse --strict`,
  the complete Rust/SQLite test suite, `prek -a`, and
  `nix flake check --no-build`; distinguish spec validation from behavioral
  verification in the final evidence

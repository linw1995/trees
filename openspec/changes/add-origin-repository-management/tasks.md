## 1. Origin Persistence and Input Resolution

- [x] 1.1 Add registration, management mode, ownership, exact URL uniqueness, and pending-clone schema with typed Snafu errors; verify legacy origins become manual without changing IDs, pool keys, or foreign keys.
- [x] 1.2 Classify create repo inputs as explicit paths, supported URLs, or directory-name fallbacks; verify colon-bearing paths, Windows paths, existing-path precedence, unique NAME lookup in both modes, unknown and ambiguous names, and unsupported inputs before side effects.
- [x] 1.3 Add transactional identity and URL resolution plus base name lookup; verify duplicate base names remain legal, manual repositories inside `origins-dir` remain manual, and linked paths preserve existing automatic ownership.

## 2. Automatic Source Provisioning

- [x] 2.1 Add `origins-dir` configuration and platform defaults; verify relative paths follow existing configuration rules, unrelated keys survive, and changes neither move existing clones nor create a root for manual registration.
- [x] 2.2 Add durable per-URL reservations and exclusive contained clone targets with process locks; verify concurrent requests cannot publish duplicate automatic origins or overwrite existing paths.
- [x] 2.3 Execute Git clone with inherited authentication and validate before atomic origin publication; verify successful fixtures, clone/HEAD failures, standard error-only progress, URL reuse across root changes, and rejection of missing or replaced retained clones.
- [x] 2.4 Implement partial-clone cleanup and abandoned-reservation recovery; inject interruption and commit-acknowledgment failures and verify only proven partial files are removed, published origins survive, and uncertain ownership retains actionable evidence.

## 3. Create Integration

- [x] 3.1 Integrate path, URL, and directory-name resolution ahead of both existing create modes without new repo or origin options; verify mixed inputs, duplicate identity detection, directory collisions, and existing `--open` and `--json` validation.
- [ ] 3.2 Preserve revision selection and pool lookup after resolution; verify repeated URLs reuse origin IDs and pools, known URLs work offline, unknown URLs fail offline before cloning, and both repo modes support both workspace modes.
- [ ] 3.3 Define and exercise failure boundaries across multi-input provisioning and workspace creation; verify invalid local inputs fail before clone, successful sources survive later failures with reported IDs, and partial worktrees follow existing rollback.

## 4. Repos Status and Id-Based Removal

- [ ] 4.1 Add status repos view and all-view parsing using existing read-only snapshots; verify missing databases, old schemas, unregistered rows, base name collisions, terminal escaping, and unchanged existing views.
- [ ] 4.2 Add the version-1 repos JSON envelope with ownership fields; verify deterministic ordering, full paths, a single snapshot timestamp, and no Git probes or operation recovery.
- [ ] 4.3 Look up target IDs across workspace and origin records; verify unknown and cross-table ambiguous IDs fail while existing workspace guards, options, and exit behavior remain intact.
- [ ] 4.4 Implement repo registration removal with existing confirmation and read-only preflight; verify both modes preserve files and references, force never deletes source data, repeated removal is a no-op, and explicit path/URL create re-registers the same identity.

## 5. Documentation and Integration Validation

- [ ] 5.1 Update README and CLI help for direct URL creation, directory-name resolution, `origins-dir`, repos status, and remove dispatch; verify examples match help and clearly describe retained clones and metadata-only repo removal.
- [ ] 5.2 Run full Rust tests and applicable repository checks, including SQL boundaries and formatting; verify create/reuse/release/status/remove/GC regressions across both origin modes and migration integrity.
- [ ] 5.3 Run strict OpenSpec validation and map all new scenarios to implementation verification before marking the change complete.

# Design

## Context

Git reports changes only inside registered worktrees. Workspace removal checks
workspace root entries against persisted worktree paths. Release can accept
degraded worktrees that need alignment, so rejecting every degraded workspace
would also reject supported clean branch changes.

## Decisions

- Extract the existing content check from root containment validation. Its
  typed error and exact path whitelist remain unchanged; callers do not need
  to invent a managed root or depend on the current configuration.
- Reconciliation marks an otherwise attached workspace degraded when shared
  content validation fails. Worktree states remain attached.
- Alignment preflight checks root content before fetching or changing worktrees.
  Filesystem failures also prevent reuse. A workspace whose root is a managed
  worktree continues to use Git cleanliness rules.
- Keep separate release and allocation regressions because their protections
  are independent. Test dangling symlinks in the existing validator test rather
  than repeating lifecycle fixtures for every entry type.

## Ablation Review

| Experiment | Observed result | Decision |
| --- | --- | --- |
| Remove reconciliation content check | Both regressions fail because shared content is not recorded as degraded. | Retain health observation. |
| Remove alignment content preflight | Release succeeds with shared content and removes the claim; the allocation regression still passes. | Retain release protection and both regressions. |
| Remove file, hidden-file, and empty-directory lifecycle variants | Focused regressions pass; the existing validator already rejects unexpected entries. | Remove repeated lifecycle cases. |
| Move dangling-symlink coverage into the existing validator test | Validator and lifecycle checks pass with less setup and no entry-type dispatch. | Retain the smaller test arrangement. |

## Risks / Trade-Offs

Any extra root entry, including an empty directory, prevents release. Managed
worktree paths remain allowed even when named `public`. Existing automatic
allocation considers persisted ready slots only; removing shared content must
be followed by reconciliation before a degraded slot can be reused.

## Migration Plan

No migration is needed. Review against repository error and visibility rules,
run regression tests and repository checks, then archive the reviewed change.
The synchronized specification passes standard validation. Strict validation
reports the same fourteen existing requirement length warnings as the
unchanged specification; this change adds no strict diagnostics.

# Disk Usage Ablation Review

The review covered every commit from `78216ad..6352e6b` in order. Each code
ablation ran in an isolated detached worktree and was restored before the next
experiment. The main worktree retained only the simplifications listed below.

| Commit | Ablation finding | Decision |
| --- | --- | --- |
| `5a1b083` | The original plan explains the live measurement contract. | Keep as archived history. |
| `b3c615f` | Disabling hard-link deduplication failed the allocated-byte test: 16,384 bytes instead of 8,192. Removing child `O_NOFOLLOW` still passed the race test because inode verification is another guard. | Keep deduplication and both filesystem guards. |
| `fe4006f` | Live status observation was removed by the later cache conversion. | No residual scan path to remove. |
| `af48924` | Omitting top-level target usage from JSON failed the report contract test. | Keep compatible JSON and human size cells. |
| `7022554` | Its read-only and output checks evolved into the current cached status test. | Keep the current integration coverage. |
| `b38b517` | Archived the initial behavior contract. | Keep the historical spec. |
| `7af8ec6` | Planned the follow-up cache contract. | Keep the archived rationale. |
| `d668f57` | Removing the origin path invalidation trigger failed its stale-cache test. Removing the full-list cache readers left production compiling but caused 24 test compile errors. | Keep the trigger and thin test-support readers. |
| `bfa68e9` | Added a user-facing refresh command that `541fa05` removed. | No command remains in the final CLI. |
| `a6c98b0` | Suppressing the top-level target cache field failed the report JSON test. | Keep the cached status projection. |
| `9702379` | Removing worktree removal invalidation failed the removal test: a pre-removal size remained. | Keep invalidation and lifecycle refresh. |
| `541fa05` | Removed the command after the user narrowed the scope. | Keep the internal-only refresh entry points. |
| `3fc4d18` | Records migration, status, and performance verification. | Keep the archived evidence. |
| `163d7bb` | Synced cache requirements into main specs. | Keep the current specification. |
| `6352e6b` | Bypassing the five-second reuse check failed the cross-connection test. Removing the scan-start log failed the create CLI test. | Keep debounce and progress logs. |

## Simplifications Applied

- Replaced four refresh counters, left over from the removed CLI command, with
  a typed complete or incomplete outcome. Per-path logs retain the detailed
  status, duration, and path-change reason.
- Considered removing the warning-helper unit test, then retained it because
  it is the only focused check that an unavailable measurement leaves the
  workspace lifecycle state unchanged.
- Removed an internal unknown-selector test duplicated by the integration test
  for an unknown workspace ID. The path selector remains protected by typed
  error handling and the existing workspace locator tests.
- Removed a second workspace from one integration fixture. The focused refresh
  unit test already covers two workspaces sharing an origin, while the remaining
  integration fixture covers source status and missing-path cache behavior.

## Verification

- `direnv exec . sh -c 'PATH=/run/current-system/sw/bin:$PATH cargo test --quiet'`
  passed after the simplification. The system Bash is needed by the existing
  shell completion tests.
- `direnv exec . prek -a` passed, including formatting, Clippy, cargo check,
  Markdown lint, static diagnostics, and runtime SQL checks.
- Strict validation passed for this change and the existing disk usage and
  workspace status specifications. `git diff --check` reported no whitespace
  errors. The final code diff removes more lines than it adds.

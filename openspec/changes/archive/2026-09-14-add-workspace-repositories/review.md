## Specification Review

The implementation was reviewed against the workspace management, reuse,
locator, and lifecycle changes. No blocking findings remain after the fixes below.

| Finding | Resolution |
| --- | --- |
| Creation intent alone could identify a foreign worktree as owned | Create target directories exclusively and record filesystem identity before Git attachment; recovery verifies ownership before adoption or deletion |
| Target lookup after recovery could adopt a replacement claim | Retain the original claim snapshot through admission |
| A rejected initial move could be recorded as completed compensation | Record failures without workspace mutation as failed operations |
| Published source details could be absent after a later clone failure | Append source publication events as origins become available |
| Public execution helpers could receive an unrecorded plan | Verify the lease and persisted plan before external mutation |

Regression coverage includes foreign and replaced target directories, locked
initial moves, unrecorded plans, and retained source details after clone failure.
Existing tests cover atomic pool publication, preserved claims, compensation
failures, missing roots, directory collisions, and the normal consumers.

Directory creation followed by a failed ownership record remains conservative:
recovery preserves that directory for inspection. This follows the specification
requirement to retain contents when ownership cannot be proven.

## CI Findings

The first Linux run passed lint, tests, coverage, and the Nix build. The CRAP
metric check identified five functions above its threshold. The workflows were
split into focused source resolution, planning, compensation, and recovery
helpers without changing the lifecycle contract or metric threshold. All 343
local tests and Clippy passed after this refactor.

## Visibility Review

The addition submodules, plan types, target lookup, and execution helpers are
private. Only the request, result, error types, and execution entry point form
the external API. Internal callers use restricted crate or parent visibility;
Snafu selectors remain private. Unused source resolution API was removed.

Tests that require internal steps now live inside the workflow module. Public
behavior remains covered by integration tests, using shared test fixtures.
The full suite still contains 343 tests, with no scenarios removed.

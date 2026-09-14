## Why

Exiting a program launched by automatic create currently leaves its workspace
claimed. Users need an optional session lifecycle that returns the workspace
to the pool and opens a recovery shell when release needs manual intervention.

## What Changes

- Add `create --release-on-exit`, requiring `--open[=<PROGRAM>]` and automatic
  allocation without a positional workspace path.
- Wait for the selected program and attempt ordinary release with the exact
  claim acquired by this invocation, including after nonzero exit or launch failure.
- On release failure, report the cause and open `$SHELL` in the workspace;
  retry the release after the shell exits, repeating while the original claim remains.
- Stop without adopting another claim when the original claim is already gone.
- Preserve the original program outcome after recovery; report unresolved
  failures with workspace identity and a manual release command.
- Keep existing create/open behavior when the flag is absent.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `workspace-management`: Add optional supervised program execution to create.
- `workspace-reuse`: Define automatic release, recovery shells, ownership checks,
  and terminal outcomes for the supervised session.

## Impact

Changes affect `src/cli.rs`, `src/main.rs`, a focused session orchestration module,
existing release APIs as needed for typed ownership classification, CLI, and
process integration tests, `README.md`, and `docs/workspaces.md`.
`UNIX` signal handling requires platform-specific supervision and terminal tests.
No storage migration or changes to standalone `trees open`, `release`, or GC
semantics are planned.

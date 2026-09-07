## Why

Creating a workspace and immediately beginning an interactive session there currently requires shell-side parsing or a separate directory change. Trees can provide a direct session handoff without attempting to mutate its parent shell.

## What Changes

- Add `--open[=<PROGRAM>]` to manual and automatic create.
- Default the opened program to `$SHELL` when `--open` has no value.
- Start the program with the workspace as its current directory and replace the Trees process where supported.
- Reject `--open` with `--json` and reject a missing or empty default shell before workspace mutation.
- Document the process and directory lifecycle when the opened program exits.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `workspace-management`: Add the create-time program handoff contract.

## Impact

The create parser and dispatch, process execution behavior, tests, README usage, and workspace-management specification change. Workspace allocation and persistence remain unchanged.

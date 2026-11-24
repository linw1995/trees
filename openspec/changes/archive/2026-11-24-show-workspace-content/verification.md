# Verification

## Automated Checks

- `cargo test --all-targets --all-features`: all suites passed.
- `prek -a` inside `nix develop`: formatting, clippy, cargo check, Markdown,
  rust-analyzer, dependency notices, runtime SQL boundary, and Harper passed.
- `nix flake check --no-build`: all evaluated checks passed.
- `openspec validate show-workspace-content --strict`: valid.

## Coverage

- Unit tests cover unexpected files and directories, managed instructions,
  entry ordering, the 20-entry bound, single-worktree roots, missing and removed
  roots, unreadable roots, and absent targets.
- Integration coverage creates a container workspace with two expected
  worktrees, verifies the `Content` line, `WORKSPACE PATH` / `KIND` table,
  persisted repository rows, modified instructions, a clean root, a
  single-worktree root, a missing root, and the JSON observation.
- Existing status tests assert that lifecycle storage is unchanged.

## Manual Verification

A container workspace with an unexpected `notes.md` and `scripts/` directory
renders:

```text
Workspace (selected by ID)
  ID          01a20c1b-7c59-7593-8072-1110ad899225
  Path        /private/tmp/trees-demo-YQs5OG/team
  Status      ready
  Mode        manual 👤
  Content     dirty (2 entries)
    WORKSPACE PATH  KIND
    notes.md        file
    scripts         directory
  Repos       2/2 api,web
    REPO  SIZE
    api   8.0 KiB
    web   8.0 KiB
  Disk usage  20.0 KiB
```

The managed `AGENTS.md` and expected `api/` and `web/` worktree paths are not
listed. A modified `AGENTS.md` appears as an unexpected file. The workspace
inventory keeps persisted labels and has no content column.

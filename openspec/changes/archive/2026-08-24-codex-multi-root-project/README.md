# Codex Multi-Root Project Launch

Completed on 2026-08-24.

This change added the managed `trees codex` and `trees codex resume` flows,
synchronized managed worktree paths with an app-server Project, created
project-bound durable threads, and handed sessions to the native Codex CLI.

The change was validated with:

```text
openspec validate codex-multi-root-project --strict
```

The launcher was removed on 2026-09-23. The proposal, design, tasks, and
capability spec remain here as the historical implementation plan. See
[retirement record](retirement.md) for the missing Codex capabilities and the
reason the narrower CLI integration was retired.

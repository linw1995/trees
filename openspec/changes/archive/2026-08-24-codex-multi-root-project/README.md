# Codex Multi-Root Project Launch

Completed on 2026-08-24.

This change added the managed `trees codex` and `trees codex resume` flows,
synchronized managed worktree paths with an app-server Project, created
project-bound durable threads, and handed sessions to the native Codex CLI.

The change was validated with:

```text
openspec validate codex-multi-root-project --strict
```

Desktop UI navigation and hosted ChatGPT App invocation remain separate
follow-up changes.

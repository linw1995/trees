# Codex Integration

[Back to Trees](../README.md#documentation)

## Start a Session

Launch an interactive Codex session for a managed workspace:

```sh
trees codex -C ./workspace
```

Trees uses the last effective `-C` or `--cd` argument to locate the workspace,
or the current directory when neither is present. Before launch, it reconciles
the workspace with Git.

The Codex project roots are the workspace's managed worktree directories in
deterministic order. Repeated launches reuse the workspace's Codex project,
synchronize its complete root list, and create a new durable thread for each
session.

After project and thread setup, Trees hands the thread to `codex resume` and
keeps the terminal attached to Codex. The setup app-server is short-lived.
Authentication, model, approval, and sandbox settings come from your normal
Codex configuration; Trees does not add unrestricted-access flags.

## Forward Codex Arguments

Trees resolves `codex` from `PATH` by default. Use `--codex-bin` to select a
custom executable:

```sh
trees codex --codex-bin /path/to/codex -C ./workspace
```

Native Codex arguments are forwarded directly without an extra `--` separator:

```sh
trees codex -C ./workspace --model gpt-5.5 --sandbox workspace-write
trees codex --model gpt-5.5
```

Forwarded `--add-dir` values are merged with the workspace's managed worktree
roots, without duplicating roots already present. Trees preserves native model,
sandbox, approval, profile, prompt, and other arguments, and appends the merged
workspace developer context needed for the multi-root handoff.

## Resume a Session

Resume an existing workspace session through the native Codex picker:

```sh
trees codex resume -C ./workspace
trees codex resume --cd ./workspace --all
```

The wrapper prepares the workspace context and invokes `codex resume` without
a thread ID, leaving session selection to the native picker. Without `-C` or
`--cd`, it uses the current directory. Pass native resume options such as `--all`
and `--last` directly.

## Verify Project Roots

From the Trees source checkout, verify project roots and an optional thread
assignment without issuing mutating Trees or Codex RPCs. Pass the managed
workspace path to the [checker script](../scripts/check-codex-project.py):

```sh
python3 scripts/check-codex-project.py /path/to/workspace
python3 scripts/check-codex-project.py /path/to/workspace --thread-id THREAD_ID
```

The checker compares Trees' managed worktree paths with the Codex Project's persisted roots. It separately checks that an optional thread's `projectId` points to that Project; it does not use `runtimeWorkspaceRoots` as a substitute for Project roots.

## Known Limitations

- A direct native `codex resume <thread-id>` does not know the Trees workspace metadata and does not automatically restore managed runtime roots or the workspace manifest. Use `trees codex resume` for the managed handoff, or pass the required `--cd` and `--add-dir` values manually.
- The native picker is scoped to the final working directory by default. Legacy sessions created with a different working directory may not appear; pass `--all` explicitly when a global picker is needed.
- The app-server Project registry and the ChatGPT app's local UI Project registry are separate. Trees does not open, select, or synchronize a Project in the ChatGPT app.
- Secondary worktree `AGENTS.md` files are not automatically discovered by Codex when another worktree is the primary instruction source. Trees injects a workspace manifest, but repository-specific secondary instructions still require explicit handling.

See [Workspace lifecycle](workspaces.md) to create a workspace, or
[Quick start](../README.md#quick-start) for a complete example.

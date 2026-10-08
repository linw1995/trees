# Multi-Worktree Workspace

This is a monorepo-style workspace of independent Git worktrees managed by Trees.
The root is not a Git repository; each child directory is a separate repository.

- Run Git commands inside the relevant repository and follow its `AGENTS.md`.
- Worktrees start detached; preserve commits on named branches.
- Keep the root limited to worktrees and this unchanged generated file.
  Store temporary files outside the workspace.
- Before finishing, check each repository with `git -C <repo> status --short --ignored`.
  Preserve valuable work and remove only disposable files you created.

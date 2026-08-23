# Trees

[![CI](https://img.shields.io/github/actions/workflow/status/linw1995/trees/CI.yaml?branch=main&label=CI)](https://github.com/linw1995/trees/actions/workflows/CI.yaml)
[![codecov](https://codecov.io/gh/linw1995/trees/graph/badge.svg)](https://codecov.io/gh/linw1995/trees)
[![License](https://img.shields.io/github/license/linw1995/trees)](https://github.com/linw1995/trees/blob/main/LICENSE)

Trees is a Rust CLI for managing coding workspaces composed of Git worktrees.

Each workspace contains one direct child worktree for every source repository. Trees records workspace lifecycle state and immutable events in a shared SQLite database.

The development environment is provided by Nix Flake.

## Install

Install from the GitHub repository with Nix:

```sh
nix profile install github:linw1995/trees#trees
```

Install from a local checkout:

```sh
nix develop
cargo install --path . --locked
```

Verify the installation:

```sh
trees --help
```

## Usage

Create a workspace from one or more Git repositories:

```sh
trees create ./workspace --repo /path/to/api --repo /path/to/web
```

Each repository becomes a direct child worktree under `./workspace`. The source repositories remain at their original paths.

Launch an interactive Codex session for a managed workspace:

```sh
trees codex ./workspace
```

Trees reconciles the workspace with Git before launching Codex. The Codex project roots are the managed worktree directories under `./workspace`, in deterministic order; the original repository paths are not used as roots. Repeated launches reuse the workspace's Codex project, synchronize its complete root list, and create a new durable thread for each session.

By default, Trees resolves `codex` from `PATH`. Use `--codex-bin` when Codex is installed at a custom path or when selecting a controlled executable:

```sh
trees codex ./workspace --codex-bin /path/to/codex
```

The setup app-server is short-lived. After the project and thread are persisted, Trees hands the thread to `codex resume` and keeps the terminal attached to Codex. This command does not open or navigate the Codex Desktop UI. Codex authentication, model, approval, and sandbox settings are inherited from the user's normal configuration; Trees does not add bypass or unrestricted-access flags.

Verify the Project roots and optional thread assignment without issuing mutating Trees or Codex RPCs:

```sh
python3 scripts/check-codex-project.py ./workspace
python3 scripts/check-codex-project.py ./workspace --thread-id THREAD_ID
```

The checker compares Trees' managed worktree paths with the Codex Project's persisted roots. It separately checks that an optional thread's `projectId` points to that Project; it does not use `runtimeWorkspaceRoots` as a substitute for Project roots.

## Development

Enter the reproducible development environment with Nix:

```sh
nix develop
```

Run the main local checks:

```sh
cargo test --all-targets --all-features
prek -a
nix flake check --no-build
```

See the [contributing guide](CONTRIBUTING.md) for the development workflow and the [security policy](SECURITY.md) for vulnerability reporting. Trees is licensed under the [Apache License 2.0](LICENSE).

## Current Scope

The current CLI provides workspace creation with detached worktrees from each repository's current `HEAD`. Branch selection, workspace deletion, and user-facing status or history commands are not part of the current command surface.

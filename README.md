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

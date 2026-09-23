# Trees

[![CI](https://img.shields.io/github/actions/workflow/status/linw1995/trees/CI.yaml?branch=main&label=CI)](https://github.com/linw1995/trees/actions/workflows/CI.yaml)
[![codecov](https://codecov.io/gh/linw1995/trees/graph/badge.svg)](https://codecov.io/gh/linw1995/trees)
[![License](https://img.shields.io/github/license/linw1995/trees)](https://github.com/linw1995/trees/blob/main/LICENSE)

Trees is a Rust CLI for managing coding workspaces composed of Git worktrees.

Create an isolated workspace from repository names or remote URLs, reuse
automatic workspaces between tasks, and open programs in managed worktrees.
Trees tracks workspace state and immutable events in a shared SQLite database.

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
trees --version
```

See [release installation](docs/builds.md#release-archives) for prebuilt binaries.

## Quick Start

Create a workspace from remote URLs, replacing these example URLs with your own:

```sh
trees create ./workspace --repo https://github.com/example/api.git --repo https://github.com/example/web.git
trees open --workspace-dir ./workspace
```

Trees reuses a matching registered source or clones and registers it when none
matches. Once registered, use the source directory names for later workspaces:

```sh
trees create --open --repo api --repo web
```

Each name must identify exactly one registered source. Run
`trees status --view repos` to inspect sources. An existing local directory takes
precedence over a registered name; see [source selection](docs/workspaces.md#select-source-repositories)
for matching rules.

Omitting the workspace path allocates a reusable automatic workspace; `--open`
starts a shell inside it. When finished, save your work on a branch or outside
the workspace, leave the worktrees clean,
then run `trees release` from that workspace to return it to the pool. Exiting
the shell alone does not release the claim. To release automatically after the
program exits and open a recovery shell if release fails, opt in with:

```sh
trees create --repo api --open --release-on-exit
trees create --repo api --open=program --release-on-exit
```

In recovery, save your work and leave the worktrees clean; exiting the recovery
shell retries release. See [automatic release](docs/workspaces.md#release-after-program-exit)
for terminal requirements and exit behavior.

To reserve a specific existing automatic workspace without changing its files,
branch, or revision, run `trees claim ./workspace`, or run `trees claim` inside
it. Use `trees release` when finished. See [explicit claims](docs/workspaces.md#claim-an-existing-automatic-workspace)
for eligibility, output, and recovery behavior.

With one repository, the workspace directory is the worktree root. With multiple
repositories, it contains one direct child worktree per repository. Existing
local repositories are also supported through `--repo /path/to/repo`; their
source directories stay at their original paths.

## Documentation

| Guide | Topics |
| --- | --- |
| [Workspace lifecycle](docs/workspaces.md) | Manual and automatic allocation, source selection, starting revisions, repository additions, explicit claims, and release. |
| [Status and opening](docs/status.md) | Pool capacity, workspace health, source inventory, JSON output, and opening a workspace. |
| [Storage configuration](docs/configuration.md) | Automatic workspace and source clone directories. |
| [Cleanup](docs/cleanup.md) | Automatic GC, workspace removal, and source record removal. |

Branch selection, repair, and user-facing history commands are not currently
available.

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

# Trees

[![CI](https://img.shields.io/github/actions/workflow/status/linw1995/trees/CI.yaml?branch=main&label=CI)](https://github.com/linw1995/trees/actions/workflows/CI.yaml)
[![codecov](https://codecov.io/gh/linw1995/trees/graph/badge.svg)](https://codecov.io/gh/linw1995/trees)
[![License](https://img.shields.io/github/license/linw1995/trees)](https://github.com/linw1995/trees/blob/main/LICENSE)

Trees is a Rust CLI for managing coding workspaces composed of Git worktrees.

Create an isolated workspace from repository names or remote URLs, reuse
automatic workspaces between tasks, and launch Codex with the managed worktrees
as project roots. Trees tracks workspace state and immutable events
in a shared SQLite database.

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

`-V` prints the package version. `--version` also prints the embedded commit,
tracked dirty status, and UTC build timestamp. Cargo uses the current time unless
`SOURCE_DATE_EPOCH` is set; Nix and release builds use the source timestamp for
reproducibility. `GIT_COMMIT_SHA` and `GIT_DIRTY` override Git detection for builds
without Git metadata. An optional `.packaged-commit` file takes precedence over
`GIT_COMMIT_SHA`; unavailable Git metadata is reported as `unknown`.

### Release Archives

The [release workflow](.github/workflows/CD.yaml) runs when a `v*` tag is pushed.
The tag must match the Cargo package version, for example `v0.1.0`.
After tests pass on all four platforms, it publishes Linux (`glibc` 2.35 or newer)
and macOS archives for x86_64 and ARM64 to
[GitHub Releases](https://github.com/linw1995/trees/releases).
Tags containing a prerelease suffix produce prereleases.

Each archive includes `trees`, `BUILD_INFO.txt`, `LICENSE`, and
`THIRD_PARTY_NOTICES.html`. Verify downloads against the release's `SHA256SUMS`,
extract the archive, and copy `trees` to a directory on your `PATH`.

## Quick Start

Create a workspace from remote URLs, replacing these example URLs with your own:

```sh
trees create ./workspace --repo https://github.com/example/api.git --repo https://github.com/example/web.git
trees codex -C ./workspace
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
the shell alone does not release the claim.

With one repository, the workspace directory is the worktree root. With multiple
repositories, it contains one direct child worktree per repository. Existing
local repositories are also supported through `--repo /path/to/repo`; their
source directories stay at their original paths.

## Documentation

| Guide | Topics |
| --- | --- |
| [Workspace lifecycle](docs/workspaces.md) | Manual and automatic allocation, source selection, starting revisions, and release. |
| [Status and opening](docs/status.md) | Pool capacity, workspace health, source inventory, JSON output, and opening a workspace. |
| [Storage configuration](docs/configuration.md) | Automatic workspace and source clone directories. |
| [Cleanup](docs/cleanup.md) | Automatic GC, workspace removal, and source record removal. |
| [Codex integration](docs/codex.md) | Starting and resuming sessions, argument forwarding, project verification, and limitations. |

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

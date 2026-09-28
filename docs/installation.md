# Installation

## Install with Nix

Install from the GitHub repository:

```sh
nix profile install github:linw1995/trees#trees
```

The Nix package installs Bash and `zsh` loaders in their standard completion
directories. They are available automatically when the shell has completion
discovery configured.

## Build from Source

From a local checkout, enter the development environment and install the CLI:

```sh
nix develop
cargo install --path . --locked
```

## Install a Release Archive

Download the archive for your platform and `SHA256SUMS` from
[GitHub Releases](https://github.com/linw1995/trees/releases). Verify the archive
against the checksums, extract it, and copy `trees` to a directory on your `PATH`.
Linux archives require `glibc` 2.35 or newer. Linux and macOS archives are
available for x86_64 and ARM64.

Each archive includes `trees`, `BUILD_INFO.txt`, `LICENSE`,
`THIRD_PARTY_NOTICES.html`, and Bash and `zsh` loaders under `completions/`.
From the extracted archive directory, you can load completion for the current
shell session:

Bash:

```sh
source completions/trees.bash
```

`zsh`:

```sh
source completions/trees.zsh
```

See [Builds and Releases](builds.md) for release workflow and build metadata.

## Verify the Installation

```sh
trees --help
trees --version
```

## Enable Shell Completion

For persistent Bash completion, add this line to `~/.bashrc`:

```sh
source <(TREES_COMPLETE=bash trees)
```

For persistent `zsh` completion, add this line to `~/.zshrc` after initializing
`compinit`:

```sh
source <(TREES_COMPLETE=zsh trees)
```

Restart the shell after changing its configuration. The shell loads a
registration from the installed binary, so completion stays compatible after
upgrades. Completion suggests command options and IDs accepted by each command.
Positional `open` and `remove` accept workspace and source repository IDs. For
`create --repo` and `add --repo`, completion also suggests unique registered
source names and local directories.

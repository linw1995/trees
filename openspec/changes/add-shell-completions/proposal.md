# Proposal

## Why

Trees exposes stable workspace, claim, and source repository IDs, but users must copy them from status output when selecting a target. Bash and `zsh` completion can make these identifiers and the existing CLI grammar discoverable without changing command behavior.

## What Changes

- Provide Bash and `zsh` completion for subcommands, options, and finite argument values.
- Offer live workspace, claim, and origin repository IDs at every CLI argument position that accepts a persisted ID, including shared named selectors and command-specific positional IDs.
- Centralize ID candidate lookup and argument bindings so new ID inputs can reuse the same completion module and missing bindings are caught by tests.
- Complete unambiguous registered repository names alongside local paths for `create --repo` and `add --repo`.
- Document shell setup and ship matching completion scripts with Nix packages and release archives.

## Capabilities

### New Capabilities

- `shell-completion`: Define supported shells, candidate scope, read-only behavior, and distribution.

### Modified Capabilities

None. Completion suggests values for existing commands without changing their accepted inputs or execution rules.

## Impact

The change affects CLI startup and argument metadata, one shared CLI completion module, read-only storage queries, shell-facing documentation, Nix installation, release packaging, and focused completion tests. It adds `clap_complete` as a dependency. No schema migration or change to normal command output is expected. This change contains planning artifacts only until implementation begins.

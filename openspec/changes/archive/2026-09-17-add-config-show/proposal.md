## Why

Trees can persist storage settings through `config set`, but users cannot inspect
their effective values from the CLI. Reading `config.toml` alone does not reveal
platform defaults or the resolved locations of relative paths. Users also need
to locate the configuration file without knowing platform-specific conventions.

## What Changes

- Add `trees config show` with an optional `--json` flag and no positional arguments.
- Print the effective workspace and origin
  directories and session hook settings as Bash-compatible, shell-quoted variable assignments, or a single JSON object
  with `--json`.

- Add `trees config path` to print the configuration lookup path without reading
  or creating the file.

- Include platform defaults when the configuration file or a setting is absent.
- Reuse existing resolution and error behavior without creating configuration,
  storage directories, or a lifecycle database.

## Capabilities

### New Capabilities

- `configuration-inspection`: Read-only inspection of supported effective settings and the configuration file location.

### Modified Capabilities

None. Existing configuration mutation and allocation behavior remains unchanged.

## Impact

Changes affect `src/cli.rs`, `src/config.rs`, `src/main.rs`, CLI integration tests,
and `docs/configuration.md`. No new dependency, migration, or network access is
required.

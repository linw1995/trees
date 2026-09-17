# Configure Storage

[Back to Trees](../README.md#documentation)

## Inspect Effective Settings

```sh
trees config show
```

The command prints the effective directories as Bash variable assignments.
For example, with Linux defaults:

```bash
workspaces_dir='/home/alice/.local/share/trees/workspaces'
origins_dir='/home/alice/.local/share/trees/origins'
```

Values include platform defaults for missing settings. Configured relative paths
resolve against the configuration file's directory, using the same resolution
rules as workspace and origin allocation. These settings describe future
allocations; changing them does not move existing storage.

Every value is shell-quoted, including paths containing spaces or single quotes.
The output can be read as Bash assignments without interpreting path contents as
commands. Variables are not exported. For example, an embedded single quote is
represented as follows:

```bash
workspaces_dir='/srv/Alice'\''s workspaces'
```

Use `--json` for one compact JSON object with the same two settings:

```sh
trees config show --json
```

```json
{"workspaces_dir":"/home/alice/.local/share/trees/workspaces","origins_dir":"/home/alice/.local/share/trees/origins"}
```

Both fields are always strings; JSON member order is unspecified. Both output
formats use the existing path display conversion, which replaces invalid Unicode
sequences. Neither format includes the configuration file location.

Inspection works outside a repository and creates no configuration file,
directory, or database. Invalid configuration produces a diagnostic on standard
error, empty standard output, and a nonzero exit status in either format.

## Locate the Configuration File

```sh
trees config path
```

This prints the configuration lookup path as plain text followed by a newline.
For example, on macOS:

```text
/Users/alice/Library/Application Support/trees/config.toml
```

The path follows the existing platform and environment rules. The command does
not read or create the file, so it succeeds even when the file is missing,
unreadable, or contains invalid TOML. It preserves the lookup path rather than
following a configuration symlink. A failure to resolve the path produces a
nonzero exit status and a diagnostic on standard error.

The output has no label or shell quoting. Quote command substitution when using
it in Bash:

```bash
config_path="$(trees config path)"
```

`--json` is available only for `config show`.

## Automatic Workspace Directory

Configure the automatic workspace content directory independently from the
lifecycle database:

```sh
trees config set workspaces-dir /absolute/path/to/workspaces
```

The configured value is persisted as an absolute path. If unset, Trees uses
the platform data-directory default.

## Source Clone Directory

```sh
trees config set origins-dir /path/to/origins
```

The default origin directory is `trees/origins` below the platform data
directory. `repository.origins_dir` in the configuration file overrides it;
relative values resolve against that file's directory. Each new clone occupies
`<origins-dir>/<origin-id>/<directory-name>`. Changing the setting affects only
new allocations and does not move existing sources. URL lookup can reuse a
matching source anywhere. Missing or identity-mismatched sources are excluded
from URL matching while their records remain visible; an unknown URL can
produce a new clone without changing those retained records.

See [Workspace lifecycle](workspaces.md) for source selection and allocation,
and [Cleanup](cleanup.md) for removing old workspaces.

## Context

`ConfigCommand` currently contains only `Set`, which accepts `workspaces-dir` or
`origins-dir`. Configuration is read from the platform-specific `config.toml`.
Missing files and missing supported keys fall back to platform data directories.
Configured relative paths resolve against the configuration file's parent;
existing configured paths are canonicalized.

## Goals / Non-Goals

Goals:

- Expose the values used for future workspace and origin allocations and session hooks.
- Expose the configuration lookup path even when the file is absent or invalid.
- Keep inspection read-only and consistent with existing consumers.

Non-goals:

- Raw TOML dumping, unknown-key inspection, or per-setting selection.
- Provenance annotations or new configuration overrides.
- Changes to path normalization, validation, setters, or public visibility rules.
- Reporting previously allocated storage or migrating it to current roots.

## Decisions

### Show Effective Values as Bash Variable Assignments

Print these variables in this order, with a trailing newline and no `export` prefix:

```bash
workspaces_dir='/home/alice/.local/share/trees/workspaces'
origins_dir='/home/alice/.local/share/trees/origins'
latest_session_hook_program=''
latest_session_hook_timeout_ms=''
```

The paths above are an illustrative Linux example. Actual paths follow existing
platform and environment rules. Only configurable settings appear in either
output format of `show`; the configuration file location is exposed separately
through `config path`.
The storage setting names match current `config set` output. Quote every value with the existing
`bash_quote` helper: surround the displayed path with single quotes and encode
an embedded single quote by closing the quoted segment, emitting an escaped
single quote, and reopening the segment. For example:

```bash
workspaces_dir='/srv/Alice'\''s workspaces'
```

The output is valid Bash assignment syntax. Spaces, dollar signs, backticks,
backslashes, and command-substitution syntax remain literal path content.
Embedded newlines remain inside the quoted value, so an assignment may span
physical lines. Print no headings, comments, or progress messages on standard output.
Reading the output as Bash assigns the variables without exporting them
or evaluating path content as commands. The displayed path conversion remains
unchanged; quoting happens only at the presentation boundary.

Showing raw file contents was rejected because it hides defaults and leaves
relative paths unresolved.

### JSON Output

`trees config show --json` prints one compact JSON object followed by a
newline, using the same resolved projection as text output:

```json
{"workspaces_dir":"/home/alice/.local/share/trees/workspaces","origins_dir":"/home/alice/.local/share/trees/origins","latest_session_hook":null}
```

Both storage fields are required strings, including when defaults apply.
`latest_session_hook` is null when unconfigured or an object with a string
`program` and integer `timeout_ms` when configured. The Bash hook variables are
empty when unconfigured. Resolve programs and timeouts through the status hook
parser without executing or locating the program; omit derived runtime values. JSON member
order is not part of the contract. Do not print labels, progress messages, or
text-format output alongside the object. Convert paths to display strings at the
presentation boundary and serialize with the existing JSON output helper, which
escapes quotes, backslashes, and control characters. As with current text output,
non-Unicode path components use lossy display conversion; JSON does not promise
lossless filesystem-name round trips.

The flag belongs to `show`; `config set` remains unchanged. Configuration errors
use the existing standard error diagnostic and failure exit code in both modes, with
empty standard output rather than a JSON error object.

### Configuration File Location

`trees config path` accepts no positional arguments or command-specific flags.
Print the path returned by `paths::configuration_path()` as plain text followed
by a newline, with no label, assignment, or shell quoting. For example on macOS:

```text
/Users/alice/Library/Application Support/trees/config.toml
```

This supports quoted command substitution in shell commands:

```bash
config_path="$(trees config path)"
```

Use the existing platform and environment lookup rules without canonicalizing
the filepath or changing those rules. Do not read, parse, validate, or create
the configuration file, inspect its existence, or open the database. Missing,
unreadable, and malformed configuration files do not prevent locating the path.
A failure to resolve the lookup path uses the existing typed path error and CLI
failure behavior, leaving standard output empty. Path display uses the existing lossy
conversion convention. `--json` remains specific to `show`.

### Resolve One Configuration Snapshot

Add an effective-configuration projection in `src/config.rs` with typed `PathBuf`
fields for the two directories and an optional typed hook configuration. The `show` projection excludes the configuration
file path; loading and error context still use it. Load the TOML table
once per inspection and resolve all settings through helpers shared with the
existing individual getters. Keep all parsing and resolution in the configuration
module, and formatting in the CLI boundary.

Preserve the current distinction that the workspace default is canonicalized
when it exists while the origin default is returned as constructed. Do not
introduce a normalization change as part of inspection. Filesystem resolution
is observational; this command does not guarantee an atomic filesystem snapshot.

### Validate Before Writing Output

Resolve every field before printing any success output. A malformed TOML file,
invalid supported setting, read error, or path-resolution error follows the
existing typed `ConfigError` and CLI failure path. An invalid unrelated setting
is ignored under the same rules as current getters; malformed TOML still fails.

The command never opens the database or invokes directory-creation helpers.
It works outside a Git repository and before any Trees storage is initialized.

## Risks / Trade-Offs

- Shared helpers could alter existing getter behavior accidentally. Compare the
  projection against individual getters across defaults, relative paths, and
  symlinks, and retain setter regression coverage.

- Separate getter calls would be simpler but could read different file versions.
  A single table load keeps all displayed settings internally consistent.

- Human-readable path display is not lossless for every filesystem name. Retain
  the current convention in both formats and document the lossy conversion.

## Validation

Cover CLI parsing, absent configuration, partial configuration, both explicit settings,
relative paths, nonexistent paths, existing symlink paths where supported, malformed
TOML, invalid section/value types, and deterministic read failures. Integration
tests must isolate platform home/configuration/data/state paths in child processes,
assert no storage creation or configuration mutation, and assert empty standard output
on configuration failure in both modes. Parse JSON to verify required string
fields, semantic parity with Bash variable values, and escaping of special path
characters. Verify Bash round trips for spaces, quotes, newlines, dollar signs,
backticks, backslashes, and command-substitution text, including that path content
cannot execute a command. For `config path`, verify platform lookup, unchanged
output for absent or malformed configuration, no file reads or writes, and empty
standard output on path-resolution failure. Run formatting, lint, and test checks after
implementation.

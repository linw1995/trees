# Configure Storage

[Back to Trees](../README.md#documentation)

## Inspect Effective Settings

```sh
trees config show
```

The command prints effective storage and session hook settings as Bash variable assignments.
For example, with Linux defaults:

```bash
workspaces_dir='/home/alice/.local/share/trees/workspaces'
origins_dir='/home/alice/.local/share/trees/origins'
latest_session_hook_program=''
latest_session_hook_timeout_ms=''
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

Use `--json` for one compact JSON object with the same settings:

```sh
trees config show --json
```

```json
{"workspaces_dir":"/home/alice/.local/share/trees/workspaces","origins_dir":"/home/alice/.local/share/trees/origins","latest_session_hook":null}
```

The two storage fields are always strings. `latest_session_hook` is `null` when
no hook is configured; its two Bash variables are then empty. When configured,
the JSON field is an object with `program` and `timeout_ms`:

```json
{"program":"/home/alice/hooks/sessions","timeout_ms":2000}
```

`program` uses the same path resolution as status; bare names remain unchanged
for lookup through `PATH`.
`timeout_ms` is a positive integer and defaults to 2000 when omitted. The Bash
variables use the `latest_session_hook_` prefix and contain the same values.
Inspection validates these settings without running or locating the executable.

JSON member order is unspecified. Both output
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

## Workspace Session Hook

Configure one executable in the existing user configuration file:

- macOS: `~/Library/Application Support/trees/config.toml`
- Linux: `${XDG_CONFIG_HOME:-$HOME/.config}/trees/config.toml`
- Windows: `%APPDATA%/trees/config.toml`

```toml
[status.latest_session_hook]
program = "/absolute/path/to/trees-sessions"
timeout_ms = 2000
```

The executable can be a compiled binary or a script with a shebang and executable
permission. Trees does not select a language or interpreter and passes no extra
command-line arguments. An `args` setting is invalid. The positive integer
`timeout_ms` is optional and defaults to 2000 milliseconds. Remove the table to
disable the hook, or skip it for one invocation:

```sh
trees status --view workspaces --no-hooks
```

A bare program name resolves through `PATH`. Relative program paths containing a
path separator resolve against the configuration directory. The executable inherits the invoking
environment and working directory.
There is no shell expansion of the program, including `~` and environment
variables; use an absolute path or a relative path such as `./hooks/sessions`.
Trees reads no hook settings from repositories or workspaces.

Only a nonempty workspace view runs the executable, for both human and JSON
output. `config show` also reads and validates the hook configuration. `--no-hooks` bypasses even malformed configuration.
The executable is user-controlled code with the user's permissions; Trees does
not enforce read-only behavior inside it. Keep it a finite metadata query without
background processes. Hook scripts must be idempotent: repeated calls must not
accumulate side effects. Use workspace paths from the request to locate workspace
data rather than assuming a particular working directory. Use absolute paths for
script resources, or locate them relative to the executable.

### Batch Protocol

Trees closes its lifecycle database connection before calling the hook once. It
writes a version-1 JSON request to standard input and closes the pipe. Only entries
in the displayed inventory are included; `--all` includes removed workspaces.
A selected target outside the inventory is not included solely for its summary.

```json
{
  "version": 1,
  "workspaces": [
    {"id": "workspace-id", "path": "/work/api"}
  ]
}
```

The executable must exit zero and write exactly one JSON response to standard
output. Each workspace result contains an ordered `sessions` array:

```json
{
  "version": 1,
  "workspaces": {
    "workspace-id": {
      "sessions": [
        {
          "agent": "example",
          "id": "session-id",
          "title": "Review workspace changes",
          "updated_at": "2026-09-17T08:00:00Z"
        }
      ]
    }
  }
}
```

Each session requires nonempty `agent`, `id`, and `title` strings and an RFC 3339
`updated_at`. The script determines association, filtering, title fallback, and
ordering. Put the session to display first: Trees does not sort by timestamp.
The workspace table shows that first session, while JSON retains the whole list.
Session metadata belongs to workspace history and is not reset by claim changes.

Return `sessions: []` when lookup succeeds with no sessions. Omit a workspace when
its information is unavailable. A null array, invalid session anywhere in a list,
ID outside the request, duplicate object key, unsupported version, or extra text
outside the JSON document invalidates the whole response. Unknown extension
fields are ignored. Write diagnostics to standard error rather than standard
output.

The total timeout covers request delivery, output collection, and process exit.
Standard output is limited to `1 MiB`; excess output terminates the hook. Trees
retains at most `16 KiB` of standard error and drains the remainder. On timeout
or output overflow, Trees terminates and reaps the child and, on `Unix`, terminates
its dedicated process group. Detached descendants are outside that guarantee;
on other platforms only direct-child cleanup is guaranteed.

Hook failures preserve the status report and successful exit code. Failed
execution can expose captured standard error in the JSON observation; avoid
writing secrets there. Successful standard error is discarded. Trees never
forwards raw child output to its own standard output or standard error.

### Executable Example

The [example hook](../scripts/examples/session-hook) is an executable
shell example using `jq`. It returns two demonstration sessions per workspace
and does not query a real agent. Install `jq`, point `program` at the executable,
and replace its data lookup with your own provider logic. Its protocol can also
be implemented by an executable written in any other language.

```sh
printf '%s\n' '{"version":1,"workspaces":[{"id":"demo","path":"/work/demo"}]}' \
  | ./scripts/examples/session-hook
```

### Codex Provider with UV

The [Codex provider](../scripts/hooks/codex-sessions) is an executable `uv` script
using only the Python standard library. It returns real session lists from local
Codex metadata. Install `uv` and Python 3.11 or newer, then install the executable:

```sh
mkdir -p "$HOME/.local/bin"
install -m755 scripts/hooks/codex-sessions "$HOME/.local/bin/trees-codex-sessions"
printf '%s\n' '{"version":1,"workspaces":[]}' | "$HOME/.local/bin/trees-codex-sessions"
```

The warm-up command verifies runtime availability. Execution uses
`uv run --script --offline` and never downloads dependencies or Python during
status. Configure the installed executable using your absolute home path:

```toml
[status.latest_session_hook]
program = "/Users/your-user/.local/bin/trees-codex-sessions"
timeout_ms = 2000
```

The provider reads `CODEX_HOME`, defaulting to `~/.codex`. For database location,
a top-level `sqlite_home` in its `config.toml` takes precedence over
`CODEX_SQLITE_HOME`; otherwise the Codex home is used. Relative configured paths
resolve against the Codex home, while relative environment paths resolve against
the hook's working directory. Profile and command-line storage overrides are not
resolved. The newest numeric `state_*.sqlite` is opened read-only; required columns
are checked before querying. No app-server starts and no transcripts are scanned.

Only sessions whose canonical working directory equals a requested workspace root
are included. Sessions started in nested repository directories are omitted. This
avoids confusing nested workspace boundaries absent from the hook request. Archived
sessions and child-agent sessions are excluded. Results use descending update time and descending session ID as the tiebreaker. Titles prefer the database name, latest valid
`session_index.jsonl` name, stored title, preview, first message, and finally
`Untitled session`. Optional name and millisecond timestamp columns are supported.

Missing storage returns empty lists. Unreadable or incompatible storage fails the
hook visibly. The provider depends on Codex's internal metadata format and may
need updates when that format changes. It never creates or migrates Codex storage.

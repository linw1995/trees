# Configure Storage

[Back to Trees](../README.md#documentation)

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
path separator resolve against the configuration directory, which is also the
child's working directory. The executable inherits the invoking environment.
There is no shell expansion of the program, including `~` and environment
variables; use an absolute path or a relative path such as `./hooks/sessions`.
Trees reads no hook settings from repositories or workspaces.

Only a nonempty workspace view reads this configuration and runs the executable,
for both human and JSON output. `--no-hooks` bypasses even malformed configuration.
The executable is user-controlled code with the user's permissions; Trees does
not enforce read-only behavior inside it. Keep it a finite metadata query without
background processes.

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

# Workspace Lifecycle

[Back to Trees](../README.md#documentation)

## Create Workspaces

### Choose Manual or Automatic Allocation

| Mode | Command | Lifecycle |
| --- | --- | --- |
| Manual | `trees create ./workspace --repo api` | Uses your path; excluded from automatic allocation and GC. |
| Automatic | `trees create --repo api` | Reuses an idle slot or allocates a generated path in a repository-set pool. |

The presence of a workspace path selects the mode; there is no `--mode` option.
Both modes create detached worktrees. The `api` examples assume a previously
registered source with that directory name.

### Select Source Repositories

Each `--repo` accepts a local path, a remote URL, or the directory name of a
registered source repository:

```sh
trees create --repo https://github.com/example/api.git
trees create --repo api
trees create ./workspace --repo api --repo /path/to/web
```

A local path registers an existing source. For a remote URL, Trees reads
`remote.origin.url` from existing, locally readable sources with matching Git
identities. One exact match is reused; multiple matches require an explicit
path. No match triggers a clone below the configured origin directory. Git
remote changes take effect immediately, without updating repository records.
Different URL spellings are not assumed equivalent.

An existing local directory takes precedence over a registered directory name.
Otherwise, a bare name must match exactly one source; use the full path when
multiple sources have the same name. Prefix local paths containing a colon with
`./` to avoid interpreting them as remote addresses.

See [Configure storage](configuration.md#source-clone-directory) for clone locations.

#### Offline Use and Clone Recovery

`--offline` can reuse a known source but cannot clone an unknown URL. If a later
input or workspace step fails, successfully registered clones remain available
for retry and their IDs are printed to standard error. Failed partial clones
are cleaned only when their ownership is proven. Retry the same URL to recover
an interrupted clone; concurrent provisioning for that URL reports that the
operation is in progress.

### Select the Starting Revision

Repository arguments may name either an upstream repository or one of its
linked workspace repos. In both cases, create resolves the upstream primary
worktree and fetches before selecting the target revision. When its current
branch tracks an upstream branch, Trees uses the fetched tracking revision;
otherwise, it falls back to the local `HEAD` of the primary worktree. The
target is created in detached mode without resetting or checking out the input
repo.

Pass `--offline` to skip fetching and select the local `HEAD` of each primary
worktree instead. The option applies to both manual and automatic create:

```sh
trees create ./workspace --offline --repo /path/to/api
trees create --offline --repo /path/to/api --repo /path/to/web
```

### Reuse an Automatic Workspace

An automatic workspace is allocated from the reusable pool for a repository
set. It does not take a workspace path; Trees reuses an idle slot or creates a
generated path below its managed workspace directory. Before granting a claim,
Trees aligns a clean reusable slot to the same selected revision. If checkout
cannot preserve existing files, create fails and does not grant the claim:

```sh
trees create --repo /path/to/api --repo /path/to/web
trees create --offline --repo /path/to/api --repo /path/to/web
```

Automatic creation prints Bash assignments: `WORKSPACE_PATH`, `POOL_ID`, and
`CLAIM_ID`. Use `--json` for a single JSON object instead. `POOL_ID` is the stable
UUID of the repository-set pool. Keep `CLAIM_ID` when automation must release
the exact claim returned by create.

### Open a Program

```sh
trees create --open --repo /path/to/api
trees create --open=codex --repo /path/to/api --repo /path/to/web
```

Pass `--open` to replace the Trees process with `$SHELL` in the created or
allocated workspace. Exiting that shell returns to the original shell in its
original directory. Use `--open=<PROGRAM>` to select another executable, such
as `--open=codex`. The program inherits the terminal and environment and starts
with the workspace as its current directory. `--open` and `--json` are mutually
exclusive. If `$SHELL` is unset or empty, provide an explicit program.

### Release After Program Exit

```sh
trees create --repo /path/to/api --open --release-on-exit
trees create --repo /path/to/api --open=program --release-on-exit
```

`--release-on-exit` requires `--open` and automatic allocation; it cannot be
combined with an explicit workspace path or `--json`. Trees remains as the
parent process, waits for the selected program, and releases the original claim.
A program that exits with a nonzero status or fails to start also triggers
a release attempt.
Without this flag, exiting the program retains the claim. Standalone
`trees open` behavior is unchanged.

Trees prefixes the child process's `PS1` with `[♻️]` in `UTF-8`
locales or `[trees:release-on-exit]` otherwise. A space follows the marker. Locale detection uses the first
nonempty value of `LC_ALL`, `LC_CTYPE`, and `LANG`, in that order. Emoji rendering
depends on terminal font support. Trees builds the prompt by
preserving an inherited `PS1` or using `$` followed by a space when it is unset. This also applies
to recovery shells and requires no shell integration. The marker is best-effort:
shell startup files or prompt themes can override it, and shells that do not
use POSIX `PS1` may not display it. The parent shell's prompt is unchanged.

Trees also sets `TREES_RELEASE_ON_EXIT=1` in the child process and each recovery
shell. This session marker is inherited by their child processes and survives
prompt overrides. It describes the supervised session, not the current
directory. Trees does not set it for create without `--release-on-exit` or
standalone `trees open`; those commands preserve any inherited value.

For [Starship](https://starship.rs/config/#environment-variable), add the
following to `~/.config/starship.toml`:

```toml
[env_var.TREES_RELEASE_ON_EXIT]
format = '[\[♻️\]](bold yellow) '
```

The module appears only when the variable is set. Starship's default prompt
already includes environment variable modules. For a custom prompt, include
`${env_var.TREES_RELEASE_ON_EXIT}` in the top-level `format`. To place the marker
before the default prompt, set this at the top of the configuration, before any
module tables:

```toml
format = '${env_var.TREES_RELEASE_ON_EXIT}$all'
```

If release fails, Trees prints the reason and opens `$SHELL -i` in the workspace.
Save your work on a branch or outside the workspace and resolve the reported
problem. Exiting the shell retries release; another failure opens another shell,
including after a nonzero shell exit. Release uses the existing Git safety
checks and never forces release or discards dirty files.

You can run `trees release` inside the recovery shell. Once the original claim
is gone, Trees finishes without releasing a replacement claim or opening another
shell. Ownership is checked before every recovery launch and again during
release admission. Another process can still explicitly release and reallocate
the workspace while a recovery shell is running; the shell does not hold an
exclusive lease.

Recovery requires terminal input and output and a nonempty, executable `$SHELL`.
If the terminal, shell, workspace directory, or ownership information is
unavailable, Trees reports the original workspace path, claim ID, and a manual
`trees release --claim-id CLAIM_ID` command. It does not loop through shells
reading redirected input. An explicit program that releases successfully does
not need `$SHELL` or an interactive terminal.

Successful recovery preserves the original program's exit code, regardless of
recovery shell exit codes. Unresolved recovery preserves an original nonzero
code and otherwise returns failure. A program startup failure remains a failure
even if release succeeds. Diagnostics go to standard error.

On `UNIX`, terminal interrupts reach the foreground child while Trees waits for
termination before releasing. A `SIGTERM` sent to Trees is forwarded to its
active child; signal termination produces exit code `128 + signal`. If child
termination cannot be confirmed, Trees retains the workspace. Killing Trees
with `SIGKILL` or losing the supervisor cannot guarantee release. Only the
launched process is tracked: use a program's foreground or wait mode when it
would otherwise detach and continue working in the background.

## Add Repositories to a Workspace

```sh
trees add --repo web
trees add ./workspace --repo web --repo /path/to/shared
trees add --workspace-id WORKSPACE_ID --repo web
trees add --workspace-dir ./workspace --repo web
trees add --claim-id CLAIM_ID --repo web --offline --json
```

Without a selector, `add` chooses the nearest registered workspace containing
the current directory. An explicit directory selects that exact workspace root.
The positional directory and the three named selectors are mutually exclusive.
An explicit miss never falls back to the current workspace.

Each `--repo` accepts the same paths, URLs, and registered source names as
`create`. New worktrees use the same detached starting revision rules.
`--offline` skips fetching and cannot clone an unknown URL. Repeated repository
identities return `already_present` without fetching or resetting existing work.
Existing branches, commits, staged changes, local files, and ignored files remain
intact. Broken associations and occupied destination paths reject the operation.

Manual workspaces need no claim. An automatic workspace must already have an
active claim; adding repositories preserves that claim and changes its pool to
the exact expanded repository set. Other workspace slots do not change.
After release, the expanded workspace can be reused for its new repository set.
The pool ID originally returned by `create` can therefore become stale.

When the original worktree occupies the workspace root, adding a second
repository moves it into a named child. For example, an `api` worktree at
`./workspace` becomes `./workspace/api`, alongside the new `./workspace/web`.
The workspace path and existing worktree ID remain stable. The command reports
the old and new repository paths. Shells may follow the moved directory;
running editors and other tools are not updated automatically.

The result reports operation, workspace, claim, and pool IDs, repository results,
and moved paths. `--json` emits one object with schema version 1; diagnostics use
standard error. A request containing only existing repositories still succeeds
and records an operation.

Each addition records its plan and mutation steps before changing worktrees.
Failure attempts to undo only the new work and restore the original directory
structure. Published source clones remain registered for retry. Interrupted
operations are recovered before a later mutation; use an explicit workspace ID
or claim ID if the workspace root is temporarily absent.

If recovery finds new user changes or cannot prove ownership, it preserves the
files and records an unresolved addition. Inspect the reported paths and repair
or save the affected content, then retry `trees add`. Unresolved additions block
further membership changes, release, and reuse. Recovery retains the original
failure history. Existing project roots refresh through the normal project
preparation flow on a later invocation.

## Release Automatic Workspaces

```sh
trees release
trees release /absolute/path/to/workspace
trees release relative/path/to/workspace
trees release --claim-id CLAIM_ID
trees release --workspace-id WORKSPACE_ID
trees release --workspace-dir ./workspace
```

Choose one release target. Named options and the positional path are mutually
exclusive, including when two inputs identify the same workspace:

| Target | Selection |
| --- | --- |
| No argument | Nearest managed workspace containing the current directory. |
| Absolute or relative path | That exact managed workspace. |
| `--workspace-id WORKSPACE_ID` | The workspace with the exact stored ID. |
| `--workspace-dir PATH` | The exact registered root, like the positional path. |
| `--claim-id CLAIM_ID` | The exact claim returned by automatic create. |

ID, path, and current-directory targets release the claim active when the workspace
is resolved. Use the claim ID in automation to target a specific allocation.
Successful release prints `workspace_id`, `workspace_path`, `claim_id`, and
`released_at` as line-oriented key-value pairs.

Release requires every managed worktree to be clean: staged, unstaged, or
untracked changes reject the entire operation. It then aligns each worktree to
its source repository's current local `HEAD` in detached mode and releases the
claim only after final reconciliation. Missing, prunable, identity-mismatched,
or failed worktrees retain the claim for repair.

Only one release can run per workspace. A concurrent attempt exits busy without
waiting or retrying.

See [Status and opening](status.md) to inspect or reopen a workspace, and
[Cleanup](cleanup.md) to remove workspaces or unused source records.

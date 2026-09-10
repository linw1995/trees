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

## Release Automatic Workspaces

```sh
trees release
trees release /absolute/path/to/workspace
trees release relative/path/to/workspace
trees release --claim-id CLAIM_ID
```

Choose one release target:

| Target | Selection |
| --- | --- |
| No argument | Nearest managed workspace containing the current directory. |
| Absolute or relative path | That exact managed workspace. |
| `--claim-id CLAIM_ID` | The exact claim returned by automatic create. |

Path and current-directory targets release the claim active when the workspace
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

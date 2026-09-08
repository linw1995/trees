# Trees

[![CI](https://img.shields.io/github/actions/workflow/status/linw1995/trees/CI.yaml?branch=main&label=CI)](https://github.com/linw1995/trees/actions/workflows/CI.yaml)
[![codecov](https://codecov.io/gh/linw1995/trees/graph/badge.svg)](https://codecov.io/gh/linw1995/trees)
[![License](https://img.shields.io/github/license/linw1995/trees)](https://github.com/linw1995/trees/blob/main/LICENSE)

Trees is a Rust CLI for managing coding workspaces composed of Git worktrees.

A single-repository workspace is itself the repository worktree. A multi-repository workspace contains one direct child worktree for every source repository. Trees records workspace lifecycle state and immutable events in a shared SQLite database.

The development environment is provided by Nix Flake.

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
```

## Usage

Create a workspace from one or more Git repositories:

```sh
trees create ./workspace --repo /path/to/api --repo /path/to/web
```

With one repository, `./workspace` is the worktree root. With multiple repositories, each repository becomes a direct child worktree under `./workspace`. The source repositories remain at their original paths.

An automatic workspace is allocated from the reusable pool for a repository
set. It does not take a workspace path; Trees reuses an idle slot or creates a
generated path below its managed workspace directory:

```sh
trees create --repo /path/to/api --repo /path/to/web
trees create --open --repo /path/to/api --repo /path/to/web
trees create --open=codex --repo /path/to/api --repo /path/to/web
trees release /absolute/path/to/workspace
trees release relative/path/to/workspace
trees release
trees release --claim-id CLAIM_ID
```

The automatic command prints Bash assignments that can be captured by a shell:
`WORKSPACE_PATH`, `POOL_ID`, and `CLAIM_ID`. Use `--json` for a single JSON
object instead. Release accepts an optional workspace directory or
`--claim-id`. An absolute or relative workspace directory selects that exact
managed workspace. With neither input, release selects the nearest managed
workspace containing the current directory. Keep the claim ID when automation
must release the exact claim returned by automatic create. Workspace-directory
and current-directory targets release the claim active when the command
resolves the workspace.

Pass `--open` to replace the Trees process with `$SHELL` in the created or
allocated workspace. Exiting that shell returns to the original shell in its
original directory. Use `--open=<PROGRAM>` to select another executable, such
as `--open=codex`. The program inherits the terminal and environment and starts
with the workspace as its current directory. `--open` and `--json` are mutually
exclusive. If `$SHELL` is unset or empty, provide an explicit program.

Concurrent release attempts use try-or-exit admission: at most one release
starts for a workspace, and another attempt exits busy without waiting or
retrying. Release first rejects the entire operation when any managed worktree
has staged, unstaged, or untracked changes. Otherwise, it aligns every clean
worktree to its source repository's current local `HEAD` in detached mode and
releases the claim only after final reconciliation. Missing, prunable,
identity-mismatched, or failed worktrees also retain the claim for repair.
`POOL_ID` is the stable UUID of the repository-set pool; its BLAKE3 hash and
canonical sorted origin repository ID set are stored internally for indexed
lookup and exact matching.

The command shape selects the management mode. An explicit workspace path is
manual and remains outside automatic allocation and GC; omitting the path is
automatic. No `--mode` option is needed. Manual workspaces keep detached
worktrees using the repository-count-based layout and are never removed by
automatic GC.

Inspect the persisted workspace inventory without changing lifecycle or Git
state:

```sh
trees status
trees status --json
trees status --view workspaces
trees status --view workspaces --all
trees open WORKSPACE_ID
trees open WORKSPACE_ID --program=codex
```

The default `pools` view reports one row per automatic repository set. Its
`CAPACITY` value is `<available>/<total>/<abnormal>`. Available counts persisted
ready, unclaimed, operation-free slots; total counts current non-reclaimed
slots; abnormal counts degraded and failed slots. Interactive terminals render
these numbers in green, blue, and red respectively. Pipelines and `NO_COLOR`
receive the same plain value without ANSI escapes. Availability is a scheduling
hint; automatic allocation still reconciles a slot before use. Repository
labels start with source-path base names and expand conflicting labels with
parent components until unique within the pool.

Use `--view workspaces` for individual manual and automatic workspaces.
`STATUS` shows workspace health and appends `🔒` when an active claim exists;
absence of the lock means unclaimed. Automatic mode is shown as `🤖`, while
manual mode is shown as `👤`. Reclaimed workspace records are hidden by default;
`--all` includes them only in this detail view. `--json` emits a versioned
snapshot for the selected view. Workspace JSON retains separate state and claim
fields plus complete current operation, path, and repo-worktree details.

The human workspace view identifies each record by stable workspace ID rather
than path. `trees open` resolves that ID and starts `$SHELL` in the persisted
canonical workspace directory; `--program=<PROGRAM>` selects another executable
without shell parsing. Automatic workspaces must already have an active claim,
while manual workspaces do not require one. Open rejects reclaimed workspaces
and retained operation leases, closes its read-only database connection before
handoff, and does not reconcile or mutate lifecycle state.

Workspace `REPOS` uses `<ready>/<total>` followed by repository labels. Ready
is the user-facing name for repo worktrees stored in the `attached` state. On
interactive terminals, ready and total are green and blue; pipelines and
`NO_COLOR` receive the same uncolored value. Ready repository labels are green,
pending labels are yellow, and problem labels are red. Non-ready repositories
also retain explicit suffixes such as `(dirty)`, `(missing)`, `(mismatch)`, and
`(error)` in plain output. Removed labels are gray and use `(removed)`.
Path-derived labels escape control characters and table delimiters before color
is applied, preventing repository names from injecting terminal output.

Status reads one consistent SQLite snapshot. It does not reconcile, recover an
expired operation, run Git, inspect workspace files, or assert that an
available workspace is currently reusable. Use `gc --dry-run` when the
question is which workspaces currently satisfy reclamation checks.

Configure the automatic workspace content directory independently from the
lifecycle database:

```sh
trees config set workspaces-dir /absolute/path/to/workspaces
```

The configured value is persisted as an absolute path. If unset, Trees uses
the platform data-directory default. Reclaim old automatic workspaces with an
explicit threshold:

```sh
trees gc --older-than 30d --dry-run
trees gc --older-than 30d --yes
trees gc --older-than 30d --force
```

Normal GC reports automatic, unclaimed, claimed, age-eligible, and
candidate counts before asking for confirmation. `--yes` skips confirmation
while keeping normal safety checks. `--force` also skips confirmation and may
remove dirty worktrees or unexpected content, but never bypasses manual,
claim, operation, root-containment, or repository-identity guards.

Remove one known workspace by the stable ID shown in
`trees status --view workspaces`:

```sh
trees remove <workspace-id> --dry-run
trees remove <workspace-id> --yes
trees remove <workspace-id> --force
```

Explicit removal accepts automatic and manual workspaces and does not apply an
age threshold. Normal mode requires a safe clean workspace. `--force` may
remove dirty worktrees or unexpected content, but it does not break an active
claim or operation and does not bypass path or repository identity guards.
Successful removal keeps the workspace and worktree records as reclaimed
tombstones.

Launch an interactive Codex session for a managed workspace:

```sh
trees codex -C ./workspace
```

Trees reads the workspace from the forwarded Codex `-C` or `--cd` argument. If neither is present, it uses the current directory. Trees reconciles the workspace with Git before launching Codex. The Codex project roots are the workspace's managed worktree directories, in deterministic order; the original repository paths are not used as roots. Repeated launches reuse the workspace's Codex project, synchronize its complete root list, and create a new durable thread for each session.

By default, Trees resolves `codex` from `PATH`. Use `--codex-bin` when Codex is installed at a custom path or when selecting a controlled executable:

```sh
trees codex --codex-bin /path/to/codex -C ./workspace
```

Native Codex arguments are forwarded directly without an extra `--` separator:

```sh
trees codex -C ./workspace --model gpt-5.5 --sandbox workspace-write
trees codex --model gpt-5.5
```

Forwarded `--add-dir` values are merged with the workspace's managed worktree roots. Trees preserves the model, sandbox, approval, profile, prompt, and other native Codex arguments while adding the workspace context required for the multi-root handoff.

The final native argument vector is also used to derive the workspace: the last effective `-C` or `--cd` value wins, and the current directory is the fallback. Trees appends managed roots that are not already present and appends the merged workspace developer context.

The setup app-server is short-lived. After the project and thread are persisted, Trees hands the thread to `codex resume` and keeps the terminal attached to Codex. This command does not open or navigate the Codex Desktop UI. Codex authentication, model, approval, and sandbox settings are inherited from the user's normal configuration; Trees does not add bypass or unrestricted-access flags.

Resume an existing workspace session through the native Codex picker:

```sh
trees codex resume -C ./workspace
trees codex resume --cd ./workspace --all
```

The `resume` wrapper does not require a thread ID. It prepares the workspace context and invokes `codex resume` without a session identifier, so the native picker remains responsible for selecting the session. Without `-C` or `--cd`, the current directory is used. Native resume options such as `--all` and `--last` can be passed directly.

Verify the Project roots and optional thread assignment without issuing mutating Trees or Codex RPCs:

```sh
python3 scripts/check-codex-project.py ./workspace
python3 scripts/check-codex-project.py ./workspace --thread-id THREAD_ID
```

The checker compares Trees' managed worktree paths with the Codex Project's persisted roots. It separately checks that an optional thread's `projectId` points to that Project; it does not use `runtimeWorkspaceRoots` as a substitute for Project roots.

## Known Limitations

- A direct native `codex resume <thread-id>` does not know the Trees workspace metadata and does not automatically restore managed runtime roots or the workspace manifest. Use `trees codex resume` for the managed handoff, or pass the required `--cd` and `--add-dir` values manually.
- The native picker is scoped to the final working directory by default. Legacy sessions created with a different working directory may not appear; pass `--all` explicitly when a global picker is needed.
- The app-server Project registry and the ChatGPT app's local UI Project registry are separate. Trees does not open, select, or synchronize a Project in the ChatGPT app.
- Secondary worktree `AGENTS.md` files are not automatically discovered by Codex when another worktree is the primary instruction source. Trees injects a workspace manifest, but repository-specific secondary instructions still require explicit handling.

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

## Current Scope

The current CLI provides manual and automatic workspace creation, explicit
acquire and release, read-only workspace status, configured automatic
workspace roots, and time-bounded automatic GC. Branch selection, repair, and
user-facing history commands are not part of the current command surface.

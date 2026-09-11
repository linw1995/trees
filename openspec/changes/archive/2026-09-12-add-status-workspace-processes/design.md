## Context

See `proposal.md` for motivation. `combined::load` currently selects the target
and inventory in one read-only SQLite transaction; `WorkspaceStatus` is shared
by the target and inventory. `run_status` immediately serializes or renders that
snapshot. The `open` command on Unix-like systems replaces its process and maintains no process registry.
Release platforms are Linux and macOS on x86_64 and ARM64.

## Goals / Non-Goals

**Goals:** Preserve persisted snapshot consistency, keep process attribution
independently testable, and report limits of observation explicitly.

**Non-Goals:** Historical launch tracking, ancestry-based ownership, CPU, or
memory sampling, ports, watch mode, full command arguments, environment values,
process control, or using process counts in `release/removal/reuse` decisions.

## Decisions

### Observe After Loading Persisted State

Load the target, inventory, and minimal registered workspace boundaries
(including removed records) in the same read-only transaction. Return boundaries
as internal context rather than JSON. End the transaction and drop the connection
before invoking a process observer once, only when a target exists. Capture
observed_at immediately before enumeration. Database snapshot_at retains its
existing meaning; `OS` enumeration is a best-effort interval, not an atomic snapshot.

Use a presentation envelope that flattens the existing Snapshot and adds
target_processes. Keep `WorkspaceStatus` and its inventory equality unchanged.
A fake observer supports integration tests without depending on host processes.
Scanning all workspace inventories separately would multiply cost and introduce
inconsistent observations, so only the selected target receives a result.

### Attribute by Current Directory

Current user means the invoking process's effective `UID`, compared with each
candidate's effective `UID`. Keep shells and independently launched programs.
Exclude the current `PID` and explicitly tracked helper `PIDs` only; do not exclude
all ancestors or all programs named trees. Enumerate processes, not threads.

Normalize the `OS`-reported absolute `cwd` to its physical path when necessary;
compare native path components against stored canonical boundaries and assign
it to the deepest matching boundary, including removed boundaries. Never scan
repository contents or require the target directory to exist. A removed target
still receives the normal observation attempt: current path ownership, not a
historical association, determines matches. Unresolved or deleted `cwd` values
are unclassified rather than guessed from display strings such as a deleted suffix.
Compare the resolved directory device and `inode` against the observed directory
identity to reject deleted paths that happen to name a different directory.

Sort matches by numeric `PID`. Keep absolute `cwd` in JSON and render a relative `cwd`
in human output, with a dot for the workspace root. Resolve paths before display
conversion; represent non-`UTF-8` text with replacement characters only at the `serialization/rendering`
boundary. Escape terminal control characters in names, paths, and issue text.

### Use a Narrow Platform Observer

Create `status/processes.rs` with a small observer interface, raw candidate data,
a pure attribution function, typed observation issues, and typed Snafu source
errors. Keep platform enumeration separate from presentation and SQLite.

Use native platform adapters through the existing locked `libc` version.
The `sysinfo` API exposes optional working directories rather than typed read
errors, which loses the distinction needed for failure classification.
Linux reads process metadata from `/proc`; macOS reads native process information
through `libproc`. Neither adapter launches helpers or collects command arguments,
environment values, CPU samples, or memory statistics.

Synchronized child tests pass on macOS ARM64 and Linux ARM64, including physical
directory attribution through a symbolic link, deleted working directories, and
omission after exit. The Linux parser and simulated process filesystem tests also
run on macOS. The complete native Linux suite passes in a temporary container
without network access or elevated privileges; see [verification results](verification.md).

Use process identity information internally where available to discard `PID` reuse
or exit races instead of combining fields from different processes. Do not retry
until the host becomes quiescent, poll, sleep, or request elevated privileges.

### Model Completeness Explicitly

Return `ProcessObservation` with observed_at, status, count, processes, and issues.
Status is complete, partial, or unavailable. Each process has `pid`, name, and `cwd`;
a missing name uses null in JSON and unknown in the table without losing a
successfully attributed process. Names alone do not determine completeness.

Complete means all enumerated eligible live candidates could be classified under
the available `OS` visibility, not a guarantee that the `OS` exposes every process.
A known exited candidate is skipped. Unknown ownership, unreadable `cwd` for an
eligible candidate, or an ambiguous race makes the result partial, even if the
candidate's workspace is unknown. Known other-user processes are excluded before
`cwd` failures affect completeness. Total enumeration failure, unavailable current
user identity, or unsupported platforms produces unavailable.

For `complete/partial`, count equals processes length; partial count is a lower
bound. For unavailable, count is null and processes is empty. Issues are aggregated
stable codes with affected candidate counts, using `user_unreadable`, `cwd_unreadable`,
`process_raced`, `enumeration_failed`, `current_user_unavailable`, or `unsupported_platform`;
global issues have null counts. Do not expose other-user candidate details.
`OS` source errors stay typed internally; serialize stable codes at the CLI boundary.
Render deterministic English reasons from these codes, not raw `OS` error strings.

### Extend the Version-2 Contract

Every view adds target_processes at the top level; it is null when no target
exists and an observation object otherwise. Existing fields and their meanings
remain unchanged. Nonfatal observation issues are structured report data, not
extra `stdout/stderr` diagnostics; argument, target, storage, and serialization
failures retain their existing nonzero behavior.

Example of the new field (other envelope fields omitted):

```json
{
  "target_processes": {
    "observed_at": "2026-09-11T06:35:00Z",
    "status": "complete",
    "count": 3,
    "processes": [
      {"pid": 1201, "name": "zsh", "cwd": "/work/api"},
      {"pid": 1248, "name": "cargo", "cwd": "/work/api/api"},
      {"pid": 1302, "name": "node", "cwd": "/work/api/web"}
    ],
    "issues": []
  }
}
```

### Render the Agreed Compact Output

```text
Workspace (current directory)
  ID          01990000-0000-7000-8000-000000000001
  Path        /work/api
  Status      ready 🔒
  Mode        automatic 🤖
  Repos       2/2 api,web
  Processes   3
    PID       NAME    CWD
    1201      zsh     .
    1248      cargo   api
    1302      node    web
  Reconciled  2026-09-11 14:32:05

Pools
REPOS    CAPACITY  UPDATED
api,web  2/3/0     14:32
```

Existing claim and mode emoji remain as specified by the base spec. An explicit
target changes only the heading.
The list is not silently truncated. Empty observations omit table headers.

```text
  Processes   0
```

```text
  Processes   2 (partial: some process working directories could not be read)
    PID       NAME    CWD
    1201      zsh     .
    1248      cargo   api
```

```text
  Processes   unavailable (process enumeration failed)
```

Multiple issue reasons are deduplicated in stable code order and separated by
semicolons. Zero confirmed matches with incomplete coverage still renders
`0 (partial: ...)`, never a bare zero. No generated color is needed in the new table.

## Risks / Trade-Offs

- `OS` visibility and permissions vary: verify real children on `Linux/macOS` and
  inject failures for deterministic completeness tests; never claim universal visibility.
- A candidate with unreadable `cwd` might be unrelated: conservatively report
  partial instead of claiming an exact target count.
- Host-wide enumeration adds latency: collect only required fields once and
  record timings on representative hosts; no target means no enumeration.
- Processes can exit or `chdir` immediately: document point-in-time observation;
  do not imply lifecycle safety or durable ownership.
- Strict JSON consumers can reject additive fields: document the extension and
  preserve version-2 existing keys and types in regression tests.

## Migration Plan

No database migration is required. Implement the observer, orchestration, output,
and tests before updating status documentation. Rolling back removes the observer
and additive output field without touching persisted data. Archive this change
only after implementation and verification, not when the planning files are ready.

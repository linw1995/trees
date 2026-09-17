## Context

See `proposal.md` for motivation. `status::report::load` already separates a read-only persisted snapshot from process observation. Workspace rendering currently consumes only the persisted snapshot. Configuration is a user-level TOML table, and `trees config set` currently accepts directory values. No existing Trees table stores agent sessions.

## Goals / Non-Goals

Goals are to preserve that separation, make hook failures visible without losing inventory, and keep agent-specific selection policy outside Trees.

Non-goals are lifecycle hooks, a general plugin framework, built-in agent adapters, a provider registry, cached titles, target-summary enrichment, session resume actions, and new configuration-setting subcommands. This change does not implement the earlier suggested Codex-specific latest-session policy inside Trees.

## Decisions

### 1. One Optional Executable in User Configuration

Use the existing configuration path and the following table:

```toml
[status.latest_session_hook]
program = "/absolute/path/to/trees-sessions"
timeout_ms = 2000
```

Only eligible workspace requests load this configuration; `--no-hooks` bypasses even malformed TOML.
A configuration read or validation failure becomes an unavailable observation. This preserves status
usefulness when an optional integration breaks. Resolve relative program paths and set child working directory to
the configuration directory. Execute the configured file directly with no additional command-line
arguments. It may be a compiled binary or an executable script with a shebang; Trees does not
choose or require an implementation language or interpreter. All request data arrives through
standard input. Do not support an `args` setting; reject it as invalid configuration rather than silently
ignoring it. A single executable keeps configuration independent of its implementation. Users
can put any interpreter invocation or startup options inside their executable wrapper.

### 2. Report-Owned Observation

Add a session-hook module under the existing status entry point. Load the persisted snapshot, close storage, collect session metadata, then observe target processes. Keep `WorkspaceStatus` unchanged and add `workspace_sessions` to `Report`. Pass the observation separately into workspace rendering through the existing summary renderer. Do not query for a target absent from the inventory or add fields to its summary.

The request is bounded by the displayed inventory, including `--all` filtering. This avoids an
unexpected global agent scan imposed by Trees; the provider chooses how it queries its sources. If a
provider needs all registered boundaries for attribution, it must obtain them independently or use
explicit agent associations. Trees does not promise working directory attribution from this request alone.
Persisting title fields was rejected because that requires refresh semantics and misses externally
started sessions.

### 3. Small Versioned Protocol

Request example:

```json
{
  "version": 1,
  "workspaces": [{"id": "workspace-id", "path": "/work/api"}]
}
```

Response example:

```json
{
  "version": 1,
  "workspaces": {
    "workspace-id": {
      "sessions": [
        {
          "agent": "codex",
          "id": "session-id",
          "title": "Fix workspace reuse",
          "updated_at": "2026-09-17T08:00:00Z"
        },
        {
          "agent": "claude",
          "id": "earlier-session-id",
          "title": "Review workspace lifecycle",
          "updated_at": "2026-09-16T08:00:00Z"
        }
      ]
    }
  }
}
```

Use typed `serde` data types and explicit validation. Reject duplicate object keys during decoding rather
than silently overwriting them in a map. Missing IDs are recoverable partial coverage, while
malformed supplied values invalidate the batch. All four session fields are required so the result
is usable as metadata beyond its rendered label. A provider with no real title may supply a
documented preview fallback. Each workspace returns a required ordered `sessions` array. An empty
array means no sessions; an omitted workspace means unavailable information. Null is invalid. Trees
validates every entry, preserves list order, and displays only index zero without sorting by
timestamp or agent. JSON retains every returned session, including entries after the displayed one.

Provider authors should normally place the latest relevant session first, preferring recently updated, non-archived main sessions and excluding child agents, but the protocol intentionally leaves filtering and ordering to the script. Different agents expose different metadata and titles.

### 4. Bounded Subprocess Supervision

Use a monotonic deadline and service all three pipes concurrently. Close standard input after the request. Account for blocking writers and readers when a child exits while descendants retain descriptors: do not unconditionally join workers that can outlive the deadline. Reuse applicable process-supervision primitives only where they preserve these semantics; the release-on-exit supervisor is not itself a hook runner.

Use a dedicated `Unix` process group for cleanup and reap the direct child. Bound retained standard output and standard error as specified. Platforms other than `Unix` implementations must bound collection and reap the direct child; descendant-tree cleanup is not promised there. Intentionally detached descendants cannot be guaranteed to terminate on `Unix` either. Document that hooks should be finite queries without background children.

Keep configuration, spawn, transport, and decoding errors as typed Snafu errors inside the implementation. Convert them into stable observation issues at the report boundary; retain bounded standard error only for failed execution. No raw child output is inherited by the terminal. Per-workspace subprocesses were rejected because they multiply startup latency and failure handling.

### 5. Additive Presentation

Keep JSON schema version 2, following the existing additive process-observation precedent.
`workspace_sessions` has its own observation time and explicit per-workspace completion state,
separating absent sessions from unavailable data. Sort issues deterministically. The human column is
conditional, preserving default output when unconfigured. Escape before display-width truncation and
retain complete escape tokens. A fixed 60-column maximum keeps output deterministic in terminals and
pipes; full session lists and values remain in JSON. Unavailable entries use an empty list together with `status: "unavailable"`, distinguishing them from successful empty results.

## Risks / Trade-Offs

- Arbitrary scripts can mutate files or access the network despite a query-oriented contract. Mitigation: load only explicit user configuration, document the trust boundary, and provide `--no-hooks`; do not claim sandbox enforcement.
- A slow hook adds latency to status. Mitigation: one batch, a 2000 milliseconds default deadline, bounded output, and no retries.
- Provider selection policies can differ. Mitigation: require provider documentation and retain agent, session ID, and timestamp in JSON; Trees makes no activity or lifecycle claims from a title.
- Strict response validation discards otherwise usable rows if one supplied row is malformed. Mitigation: document this atomic validation rule and allow omission for unavailable workspaces.
- Captured standard error may contain provider diagnostics with sensitive content. Mitigation: retain at most 16 `KiB` only for failures and do not render raw standard error in human output; provider documentation must explain that failed standard error is exposed in JSON.

## Migration Plan

No database migration or existing configuration rewrite is required. Ship configuration/status documentation and a small example provider that demonstrates the protocol without reading real agent storage. Existing installations remain unchanged until configured. Removing the configuration table or passing `--no-hooks` restores the previous human view. Validate with fake executable providers. Real agent integration is a separate change.

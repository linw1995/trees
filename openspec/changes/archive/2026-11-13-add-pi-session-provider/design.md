## Context

Trees invokes one user-configured executable for session metadata. The existing
provider reads Codex's versioned SQLite metadata and returns workspace-scoped
session lists. Pi stores sessions as JSON Lines (`.jsonl`) files grouped by working
directory under its agent directory, with optional storage overrides.

## Goals / Non-Goals

Read Pi's persisted session headers and active metadata without mutating storage,
then merge the results with Codex sessions in the existing response. Do not run Pi,
start an agent process, scan unrelated data, or require a Pi installation.

## Decisions

- Determine the Pi agent directory from `PI_CODING_AGENT_DIR`, defaulting to
  `~/.pi/agent`. Determine session storage from `PI_CODING_AGENT_SESSION_DIR`, then a
  top-level `sessionDir` in the agent directory's `settings.json`, then the
  `sessions` subdirectory. Resolve relative paths against the hook working
  directory. Do not resolve `--session-dir` or project settings because the hook
  cannot observe those invocation decisions reliably.
- Discover `.jsonl` files recursively below the session root. Read the first valid
  session header, skip files without a usable absolute `cwd` and string `id`, and
  only scan the remaining lines when the canonical `cwd` equals a requested root.
  This keeps nested-directory sessions out for the same reason as the Codex
  provider.
- Use Pi's own listing policy for the first displayed fields: the latest
  `session_info` name, the first nonempty user message text, and then
  `Untitled session`. Use the latest user or assistant message timestamp, then the
  header timestamp, then the file modification time for ordering.
- Include forked and cloned sessions because they are ordinary Pi session files
  with a `parentSession` header field. Skip malformed lines and individual files
  that cannot be read, matching Pi's fault-tolerant session listing.
- Merge both providers by descending update time, then descending session ID. A
  missing provider store contributes no sessions; invalid Pi settings fail the
  hook visibly so configuration errors are not hidden. Never create or migrate
  agent storage.

## Risks / Trade-Offs

- Pi session files are an internal format. Tolerant parsing avoids breaking status
  on truncated appends, but future format changes may require updates.
- Project-level `sessionDir` settings and `--session-dir` are not resolved. Document
  the limitation and rely on the environment or agent settings for custom storage.
- Scanning every session header adds work proportional to stored sessions. Reading
  full files only for matching workspaces keeps the common case bounded.

## Migration Plan

Update the existing provider and documentation. Existing Codex-only configurations
keep working because the executable path and hook protocol do not change. Remove
the hook table to disable both sources. No database migration is required.

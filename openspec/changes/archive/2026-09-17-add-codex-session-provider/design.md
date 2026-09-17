## Context

Trees already accepts one executable that exchanges version-1 JSON and returns ordered session lists. Local Codex metadata includes a versioned SQLite database and an optional append-only name index.

## Goals / Non-Goals

Provide a dependency-free Python provider executable through `uv`, with a real local status check. Do not start an app-server, scan conversation transcripts, migrate databases, or infer ownership from repository names.

## Decisions

- Use an executable script with inline Python metadata and `uv run --script --offline`. A compatible Python installation must exist before interactive status calls; no dependency download is needed.
- Resolve `CODEX_HOME`, then top-level `sqlite_home`, then `CODEX_SQLITE_HOME`, then the Codex home directory. Relative configuration paths resolve from the Codex home; relative environment paths resolve from the invoking directory. Layered profile and command-line overrides are outside this provider.
- Select the highest numeric `state_*.sqlite` version and validate required columns. Missing storage returns empty lists; incompatible or unreadable storage fails visibly. Do not silently fall back to an older database.
- Match canonical session working directories exactly against requested workspace roots. This avoids ownership errors when removed or nested workspace boundaries are absent from the request. Sessions started inside repository subdirectories are outside this initial policy.
- Exclude archived sessions and child-agent sources. Sort by update time descending, then session ID descending. Prefer a database name, the latest indexed name, stored title, preview, first user message, and finally `Untitled session`.
- Support optional name and millisecond timestamp columns, preserving compatibility with older metadata schemas. Query with SQLite read-only mode and a short lock timeout. Ignore incomplete name-index lines as Codex does.

## Risks / Trade-Offs

- Codex metadata is an internal format. Validate its required fields, document the coupling, and fail without modifying it.
- Exact root matching omits nested-directory sessions. This is deliberate until the provider has a complete source of workspace boundaries.
- Offline execution needs a compatible local Python runtime. Warm up the script during installation and keep the existing hook timeout configurable.

## Migration Plan

Install the executable at a stable user path, preserve existing Trees configuration, and set the hook program to that path. Remove the hook table to disable integration. No database migration is required.

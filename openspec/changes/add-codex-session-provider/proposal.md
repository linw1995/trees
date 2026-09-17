## Why

The session-hook protocol needs a usable Codex provider so workspace tables can show real conversation titles. A standalone executable keeps Codex storage compatibility outside the Rust status implementation.

## What Changes

- Add an executable `uv` script using only the Python standard library.
- Read Codex metadata through a read-only SQLite connection and return ordered session lists.
- Document installation, configuration, selection policy, and storage compatibility.
- Run the locally built Trees binary against local Codex metadata.

## Capabilities

### New Capabilities

- `codex-session-provider`: Read-only Codex session metadata for the existing workspace hook protocol.

### Modified Capabilities

None.

## Impact

Adds a provider script and configuration documentation. Rust behavior and lifecycle storage remain unchanged. Local activation installs a copy of the executable and updates only the user's hook configuration.

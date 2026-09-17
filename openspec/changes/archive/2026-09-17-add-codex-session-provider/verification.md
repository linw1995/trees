## Local Verification

The provider was installed at a stable user executable path and enabled in the
local Trees configuration, preserving repository and workspace directory settings.
The installed script was compared byte-for-byte with the repository copy.

Using the locally built `target/debug/trees` against real Codex metadata:

- All 18 requested workspaces returned complete observations, totaling 70 sessions.
- The current workspace returned 9 sessions, and its first title appeared in the table.
- JSON completed in 164 milliseconds; human output completed in 188 milliseconds.
- The persisted workspace inventory matched the result from `--no-hooks`.
- Standard error was empty and the observation had no issues.

The executable's initial direct invocation completed in 382 milliseconds. The
existing 11 status integration tests passed after the local build. No new test
files are included, as requested; validation used the actual executable and data.

## Final Specification Review

Temporary SQLite fixtures exercised the repository script without reading personal
agent data or adding test files. Checks passed for missing storage, canonical exact
roots, archived and child exclusion, deterministic ordering, name precedence,
renames, preview and untitled fallback, and millisecond timestamps. Repeated calls
from different working directories returned identical output and preserved all
fixture storage hashes. An incompatible highest-version database failed without
partial JSON, fallback, or storage mutation.

## Local Verification

The provider was checked with temporary Codex and Pi fixtures and with local user
metadata. Fixtures used a temporary `HOME`, `PI_CODING_AGENT_DIR`, and
`CODEX_HOME`; no personal session data was modified.

- Pi default storage, `PI_CODING_AGENT_SESSION_DIR`, and agent `settings.json`
  `sessionDir` discovery each returned only the expected sessions.
- Invalid Pi settings exited nonzero with the diagnostic on standard error.
- Exact-root matching excluded a session whose header `cwd` was a child of the
  requested workspace. A malformed trailing line did not suppress the valid entry.
- A named Pi session used the latest `session_info` name. A session without a name
  used its first nonempty user message, and an empty session used
  `Untitled session`.
- A fixture containing both Codex and Pi sessions produced one list ordered by
  descending update time with `codex` and `pi` agent labels. Removing either store
  left the other provider's sessions intact.
- Pi update times followed the latest user or assistant message timestamp, then the
  header timestamp, then the file modification time.
- A recursive checksum comparison before and after provider execution showed no
  storage changes.

Using the locally built `target/debug/trees` and the installed provider against
real local metadata:

- `trees status --view workspaces --json` completed in 375 milliseconds with
  `workspace_sessions.status` equal to `complete` and empty standard error.
- Workspaces with Pi sessions returned Pi titles, and workspaces with Codex
  sessions kept their existing Codex titles.
- Human `trees status --view workspaces` displayed `pi: ...` in `LATEST SESSION`
  for a workspace whose newest session came from Pi.

`openspec validate add-pi-session-provider --strict` passed.
`markdownlint-cli2` reported no errors for the changed documentation and planning
artifacts.

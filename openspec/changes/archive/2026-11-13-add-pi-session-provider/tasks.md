# Tasks

## 1. Pi Session Provider

- [x] 1.1 Extend the executable provider with Pi agent-directory and session-root discovery, including environment and agent-settings overrides; verify missing storage, environment precedence, and invalid settings behavior.
- [x] 1.2 Add read-only Pi session parsing with exact workspace-root matching, title fallback, update-time fallback, and tolerant malformed-line handling; verify named sessions, first-message fallback, nested-directory exclusion, and truncated trailing lines.
- [x] 1.3 Merge Codex and Pi sessions into the existing version-1 response with deterministic ordering and agent labels; verify both sources coexist and either missing source leaves the other intact.

## 2. Documentation and Validation

- [x] 2.1 Document Pi storage discovery, selection policy, merging, limitations, and read-only behavior in the configuration guide; verify the description matches the executable.
- [x] 2.2 Run the provider against real Pi and Codex metadata and temporary fixtures, then validate the OpenSpec change with `openspec validate add-pi-session-provider --strict`.

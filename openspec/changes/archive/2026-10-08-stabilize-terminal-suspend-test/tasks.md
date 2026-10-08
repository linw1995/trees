# Tasks

## 1. Implementation

- [x] 1.1 Confirm that the descendant helper can consume the shell command.
- [x] 1.2 Reuse the helper for a signal-based resume handshake.
- [x] 1.3 Include the expected marker in terminal timeout diagnostics.

## 2. Ablation and Review

- [x] 2.1 Remove the signal handshake and confirm the original failure remains detectable.
- [x] 2.2 Compare thread-local signal waiting and remove early handler installation.
- [x] 2.3 Remove unnecessary configuration and assess overlap with existing tests.
- [x] 2.4 Review the final diff against `AGENTS.md` and record the evidence.

## 3. Validation and Archive

- [x] 3.1 Run Linux and macOS tests, repository checks, and strict change validation.
- [x] 3.2 Archive the reviewed tooling change and validate its final record.
- [x] 3.3 Commit the final implementation and archive.

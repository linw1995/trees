## 1. Release Contract

- [x] 1.1 Define clean preflight, origin `HEAD` alignment, and failure behavior; verify the change with strict OpenSpec validation

## 2. Git Alignment

- [x] 2.1 Add a heartbeat-aware detached checkout helper and verify it preserves a clean worktree at the requested revision
- [x] 2.2 Align release worktrees only after all worktrees pass clean preflight and verify final reconciliation gates claim removal

## 3. Verification and Documentation

- [x] 3.1 Cover clean changed and dirty release behavior with workspace tests and verify the affected test suite passes
- [x] 3.2 Update user documentation and verify formatting and repository checks pass

## 1. Release Target Contract

- [x] 1.1 Add mutually exclusive workspace-directory and claim-identifier CLI targets with a current-directory default and verify parser tests accept omitted and relative directory targets while rejecting combined targets
- [x] 1.2 Resolve every target to a stable workspace path and active claim snapshot and verify workspace tests cover exact paths, descendant current directories, claim lookup, and unclaimed targets

## 2. Fail-Fast Release Admission

- [x] 2.1 Add a try-once operation-admission transaction for release and verify lock contention returns busy without waiting
- [x] 2.2 Route every release target through the existing reconciliation and atomic claim-release workflow and verify dirty and reusable workspace behavior remains intact

## 3. Documentation and Verification

- [x] 3.1 Update README release examples and semantics and verify Markdown checks pass
- [x] 3.2 Run strict OpenSpec validation, the complete Rust test suite, repository hooks, and the Nix flake check

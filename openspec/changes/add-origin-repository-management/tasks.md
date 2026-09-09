## 1. Simplified Contract

- [x] 1.1 Update the proposal, design, and specifications to use existing origin fields and reference-checked removal; verify strict OpenSpec validation.

## 2. Implementation Correction

- [ ] 2.1 Remove the four origin columns, mode type, soft deletion, and cached URL lookup; use Git remote inspection, minimal repos snapshots, and transactional reference guards, with regression tests for existing schemas, changed and ambiguous URLs, and referenced origins.

## 3. Documentation and Validation

- [ ] 3.1 Update CLI help, README, and verification evidence; run full Rust tests, repository hooks, Flake validation, and the complexity gate before committing the correction.

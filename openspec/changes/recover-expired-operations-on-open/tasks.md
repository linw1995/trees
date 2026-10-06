# Tasks

## 1. Implementation

- [x] 1.1 Add shared recovery with a worktree-preserving access policy.
- [x] 1.2 Trigger access recovery outside open selection transactions and recheck admission.
- [x] 1.3 Document eligible operations, explicit structural recovery, and ownership preservation.
- [x] 1.4 Cover successful recovery, rejected operations, concurrent admission, and persistence failure.

## 2. Ablation Review

- [ ] 2.1 Commit the verified implementation before isolated experiments.
- [ ] 2.2 Ablate the type guard and final admission check and record outcomes.
- [ ] 2.3 Simplify policy and test scaffolding while retaining unique behavior coverage.

## 3. Final Review and Archive

- [ ] 3.1 Review the final diff and run complete tests and repository checks.
- [ ] 3.2 Validate and archive this change, then verify the synchronized specification.
- [ ] 3.3 Commit the reviewed simplification and archive.

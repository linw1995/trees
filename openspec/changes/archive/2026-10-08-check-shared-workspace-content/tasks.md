# Tasks

## 1. Implementation and Review

- [x] 1.1 Reuse root content validation in reconciliation and alignment preflight.
- [x] 1.2 Cover public content preservation, rejected release, skipped allocation, and recovery after reconciliation.
- [x] 1.3 Review typed errors, visibility, claim preservation, and single-worktree behavior against repository guidelines.

## 2. Ablation

- [x] 2.1 Remove each check independently and verify the relevant regressions fail.
- [x] 2.2 Remove repeated lifecycle cases and move symlink coverage into the existing validator test.
- [x] 2.3 Verify the smaller tests still detect both independent omissions.

## 3. Validation and Archive

- [x] 3.1 Run final tests, repository checks, and strict change validation.
- [x] 3.2 Archive the reviewed change and validate the synchronized specification.
- [x] 3.3 Commit the final implementation and specification archive.

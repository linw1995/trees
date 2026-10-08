# Tasks

## 1. Implementation and Review

- [x] 1.1 Generate shared instructions during creation, expansion, and reuse.
- [x] 1.2 Preserve repository files and recognize generated content during validation and cleanup.
- [x] 1.3 Include the template in Nix package sources.
- [x] 1.4 Review errors, visibility, claims, rollback, and interrupted additions against repository guidelines.
- [x] 1.5 Reproduce and fix deletion of existing instructions after failed container creation.

## 2. Ablation

- [x] 2.1 Remove optional journal writes and confirm behavior remains correct.
- [x] 2.2 Remove protection and cleanup checks independently and confirm regression failures.
- [x] 2.3 Merge repeated lifecycle coverage into existing creation and file-validation tests.

## 3. Validation and Archive

- [x] 3.1 Run final tests, repository hooks, and strict change validation.
- [x] 3.2 Archive the reviewed change and compare synchronized spec diagnostics with the baseline.
- [x] 3.3 Commit the implementation and specification archive.

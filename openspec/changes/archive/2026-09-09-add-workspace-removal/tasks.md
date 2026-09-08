## 1. Removal Workflow

- [x] 1.1 Add single-workspace read-only preflight for both management modes
- [x] 1.2 Add serialized execution using the existing physical removal and
  tombstone persistence path
- [x] 1.3 Preserve hard safety guards and recover expired operations before
  execution

## 2. CLI and Documentation

- [x] 2.1 Add `trees remove <workspace-id> [--dry-run] [--yes] [--force]`
- [x] 2.2 Add confirmation, warnings, result output, and README usage

## 3. Verification

- [x] 3.1 Cover manual and automatic success, dry-run, force, claims,
  operations, unknown IDs, reclaimed IDs, and physical safety failures
- [x] 3.2 Run Rust checks, repository hooks, strict OpenSpec validation, and
  Nix flake evaluation

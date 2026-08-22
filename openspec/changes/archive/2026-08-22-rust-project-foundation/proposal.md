## Why

The repository needs a reproducible Rust project foundation before product behavior is finalized. Establishing the toolchain and package boundaries now allows the eventual CLI design to be reviewed independently from implementation details.

## What Changes

- Keep a minimal Cargo package with a placeholder binary.
- Manage Rust, Cargo, formatting, linting, Git, and build dependencies through the Nix Flake.
- Add project documentation and ignore rules for Rust/Nix development artifacts.
- Add Coco-style pre-commit checks, including Markdown linting.
- Add Harper prose checking with a project dictionary for tool-specific terms.
- Add GitHub Actions checks for repository linting and the Flake package build.
- Do not implement workspace, Git worktree, manifest, or CLI behavior in this change.

## Capabilities

This is a project-foundation change and intentionally sets `skip_specs: true`. Product capabilities remain subject to review in a later OpenSpec change.

### New Capabilities

None.

### Modified Capabilities

None.

## Impact

- Adds the Cargo and Flake entry points required for development.
- Does not introduce a user-facing command contract or runtime behavior.

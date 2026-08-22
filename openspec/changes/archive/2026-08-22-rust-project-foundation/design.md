## Context

The repository is an empty project that needs a Rust and Nix Flake foundation. The product behavior has not completed review, so this change must stop at package and toolchain scaffolding.

## Goals / Non-Goals

**Goals:**

- Provide a conventional Cargo package and placeholder binary.
- Provide a locked Nix Flake development shell and package definition.
- Keep generated build artifacts and local editor files out of Git.

**Non-Goals:**

- Implementing CLI argument parsing or commands.
- Invoking Git or creating worktrees.
- Defining or finalizing workspace manifests, branches, rollback, or output formats.
- Adding product tests before the behavior spec is approved.

## Decisions

### Keep the Binary Intentionally Empty

`src/main.rs` is only a compilable placeholder. This proves the package wiring without creating a behavior contract that could constrain later spec review.

### Use the Flake as the Development Boundary

`flake.nix` and `flake.lock` provide Rust, Cargo, rustfmt, Clippy, Git, and the reproducible package build. `Cargo.lock` is committed for future Rust dependency changes.

### Keep the Flake Layered

The root `flake.nix` only declares inputs and delegates to `nix/outputs.nix`. System-specific output composition lives in `nix/outputs.nix`; Rust package construction lives in `nix/packages.nix`; development shells live in `nix/dev-shells.nix`; and Fenix toolchain definitions live in `nix/rust.nix`. This keeps future package or toolchain changes local without growing the root entry point.

### Keep the Repository Surface Small

Only package metadata, the placeholder source, toolchain files, documentation, and ignore rules belong in this change. Feature modules and integration tests wait for an approved behavior spec.

### Use Lint Hooks for Foundation Quality

`.pre-commit-config.yaml` provides general file checks, Rust formatting/compile/lint checks, `markdownlint-cli2`, and Harper. The Flake development shell provides `prek` and Harper, while `.markdownlint.yaml` keeps the Markdown rules compatible with OpenSpec's structural headings and long-form planning text. `.harper-dictionary.txt` holds project-specific technical vocabulary and is also the default workspace dictionary path for Harper-LS.

### Keep CI Small and Flake-Centered

GitHub Actions uses a local `setup-nix` composite action with runner platform and architecture-aware Nix/Cargo caches. The workflow has separate `lint` and `nix-build` jobs: lint runs `prek -a`, and the build job runs `nix build` followed by `nix flake check`. Coverage, release, and product behavior jobs remain out of scope for the foundation.

## Risks / Trade-Offs

- [The empty binary is not useful by itself] → This is intentional; its purpose is to validate the project foundation while behavior remains under review.
- [Future spec decisions require changing the package layout] → Keep the current layout conventional and avoid premature module boundaries.

## Migration Plan

Use `nix develop` for the managed environment and add product modules only in a later approved OpenSpec change. No existing data or runtime behavior is migrated by this change.

## Open Questions

Product behavior and CLI shape remain intentionally open for review.

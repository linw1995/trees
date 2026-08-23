# Contributing

## Development Environment

Trees uses Nix Flake to provide the Rust toolchain and development tools. Enter the development shell before running project commands:

```sh
nix develop
```

If direnv is enabled, entering the repository loads the same development shell automatically.

## Local Checks

Run the complete local hook suite before submitting a change:

```sh
prek -a
```

Run the Rust test suite and the Flake check when changing application or build code:

```sh
cargo test --all-targets --all-features
nix flake check --no-build
```

The hook suite includes formatting, clippy, cargo check, rust-analyzer diagnostics, Markdown linting, dependency notice generation, the runtime SQL boundary check, and Harper.

## OpenSpec Changes

Behavioral changes should be planned through OpenSpec. Keep the behavior contract, technical design, implementation tasks, and verification evidence aligned with the code.

## Pull Requests

Keep changes focused and describe the user-visible behavior, implementation boundaries, and verification performed. Update documentation and dependency disclosures when the corresponding project behavior or dependency set changes.

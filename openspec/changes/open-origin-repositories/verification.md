# Verification

- `cargo test --all-targets --all-features`: passed.
- `cargo test --test workspace_open --test workspace_locator_cli`: all six tests passed.
- `nix flake check --no-build`: passed for the current system.
- Repository hooks: Rust formatting, Clippy, compilation, analyzer diagnostics,
  Markdown linting, dependency notices, and SQL boundary checks passed.
- Integration tests verify the working directory and program selection. They also
  cover explicit workspace selectors, ambiguous identifiers, and missing directories.

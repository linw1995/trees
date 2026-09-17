# Configuration Inspection Verification

## Environment

- Native macOS on Apple Silicon (`Darwin arm64`).
- Rust and Cargo 1.98.0.
- Linux and Windows were not executed in this environment. The CLI fixtures
  isolate the platform home, configuration, data, and state paths for each
  supported platform. Bash and symlink cases are enabled under `cfg(unix)`.

## Results

- `cargo test`: 385 tests passed, zero failed, and zero ignored.
- Configuration unit tests: seven passed, including three new projection tests.
- Configuration CLI integration tests: ten passed.
- `cargo fmt --check`: passed.
- Commit hooks passed for each implementation commit, including
  `cargo clippy --workspace --all-targets --all-features -- -D warnings`,
  `cargo check --workspace --all-targets --all-features`, Rust Analyzer
  diagnostics, the runtime SQL boundary check, and third-party notice validation.
- `openspec validate add-config-show --strict`: passed.
- `git diff --check`: passed.

## Behavioral Coverage

- Missing and partial configuration, relative paths, defaults, and both setters.
- Existing symlink resolution and unchanged workspace/origin default behavior.
- Typed parse, validation, and filesystem errors; failures leave standard output
  empty in both display modes.
- Bash round trips preserve spaces, quotes, backslashes, newlines, tabs, dollar
  signs, backticks, and command-substitution text without executing path content.
- JSON values match Bash values and include only the two supported settings.
- Inspection outside a repository leaves configuration and storage unchanged.
- Path lookup succeeds for absent or malformed files and even when the lookup
  location is a directory, proving it does not attempt to parse configuration.
- Path lookup preserves spaces and does not follow a configuration symlink.
- Missing platform path inputs fail with existing CLI diagnostics.
- Unsupported arguments are rejected; `--json` belongs only to `show`.

## 1. Effective Configuration

- [x] 1.1 Add a typed effective-configuration projection and load its TOML table once; share directory and hook resolution with existing getters while preserving their behavior and typed error chains.
- [x] 1.2 Cover absent/partial configuration, explicit paths, relative paths, nonexistent paths, symlinks where supported, unrelated keys, invalid types, parse errors, and read failures.

## 2. CLI Integration

- [x] 2.1 Add `ConfigCommand::Show` with `--json` and `ConfigCommand::Path`, help text, and parser coverage; adjust existing irrefutable `Set` matches for the new variants.
- [x] 2.2 Resolve the full projection before printing the ordered Bash variable assignments using `bash_quote` or serializing a single JSON object; preserve existing `config set` behavior and CLI failure handling.
- [x] 2.3 Add isolated CLI integration coverage for defaults, configured values, errors with empty standard output, execution outside repositories, and absence of configuration/storage mutations in both modes. Verify JSON field types, Bash/JSON value parity, and escaping; verify Bash round trips for special characters and that command-like path content cannot execute.
- [x] 2.4 Implement `config path` using `paths::configuration_path()` and plain path output; avoid configuration reads, existence checks, `canonicalize` calls, and storage initialization.
- [x] 2.5 Cover `config path` platform lookup, absent/malformed/unreadable configuration, no storage mutation, rejection of unsupported arguments including `--json`, and path-resolution failures with empty standard output.

- [x] 2.6 Include hook program and effective timeout in both formats, with empty Bash values or JSON null when absent. Verify validation errors, shared path resolution, safe quoting, and inspection without executing a provider.

## 3. Documentation and Verification

- [x] 3.1 Document `trees config show`, `--json`, `trees config path`, examples of Bash assignments and JSON, shell quoting and non-exported variables, effective/default semantics, plain lookup-path output even for missing or invalid configuration, and read-only behavior in `docs/configuration.md`.
- [x] 3.2 Run `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`, and strict OpenSpec validation; record results and platform limitations.

All tasks are complete. See [verification results](verification.md) for checks
and platform coverage.

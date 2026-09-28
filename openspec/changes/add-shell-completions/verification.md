# Verification

## Checks

- `nix develop --command cargo test --locked --all-targets --all-features --quiet`: all Rust unit and integration tests passed, including nine shell completion integration tests.
- `nix develop --command prek -a`: all repository hooks passed, including formatting, Clippy, compiler checks, Rust Analyzer, Markdown linting, dependency notices, and the runtime SQL boundary check.
- `nix build .#trees --no-link --print-out-paths --quiet`: passed on Darwin ARM64. The output contains `share/bash-completion/completions/trees` and `share/zsh/site-functions/_trees`, byte-for-byte matching the source loaders.
- `bash scripts/package-release.sh v0.3.0 aarch64-apple-darwin`: passed using the Nix-built binary and its generated notices. The archive contains the binary, metadata, licenses, and both shell loaders. Bash and `zsh` successfully loaded completion from the extracted archive.
- `openspec validate add-shell-completions --strict`: passed.

## Scenario Review

- CLI grammar and finite values: `cli::completion` tests exercise commands, options, status views, configuration settings, and directory hints. Shell integration tests load both registrations.
- All persisted ID positions: the binding coverage test matches every `ID` or `*_ID` argument in the derived CLI tree. Integration tests exercise each current named and positional ID argument with isolated storage.
- Candidate scope: tests cover removed workspace visibility in status, workspace-only selectors, active claims, origin IDs, cross-entity collisions, prefix filtering, and deterministic order.
- Repository inputs: tests cover unique names, ambiguous names, local directory precedence, URL-shaped input, spaces, quotes, and unsafe control characters. Generated shell registrations contain no persisted source names.
- Read-only behavior: missing and corrupt storage produce quiet empty candidates. A configured status hook and a fake Git executable are not run during completion. Normal CLI storage errors still surface.
- Distribution: Nix output paths and release archive contents were inspected. Bash and `zsh` loaders were sourced, and the standard `zsh` autoload file handled its first completion request.

## Platform Scope

Local runtime and package checks ran on Darwin ARM64. Linux behavior is covered by the same Rust tests when CI runs on Linux, but no Linux package was built locally.

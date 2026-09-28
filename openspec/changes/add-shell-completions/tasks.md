# Tasks

## 1. Shell Entry Point and Grammar

- [x] 1.1 Add the locked `clap_complete` dependency and the `TREES_COMPLETE` entry point using a shared completion command factory before ordinary CLI parsing; verify Bash and `zsh` registrations can be generated without opening or creating lifecycle storage.
- [x] 1.2 Add command, option, finite-value, and workspace-directory completion metadata through the shared factory; verify focused tests cover subcommands, `status --view`, `config set`, and path-valued selectors in both shells.
- [x] 1.3 Document Bash and `zsh` configuration commands in README; verify the documented commands register completion in clean shell sessions.

## 2. Persisted Identifier Candidates

- [x] 2.1 Add one shared ID completion module with focused read-only queries. Cover all workspaces, non-removed workspaces, active claims, origins, and cross-entity positional IDs. Verify prefix, sorting, deduplication, removed-state, and collision tests against a temporary database.
- [x] 2.2 Register every positional and named ID argument in the spec table, including flattened locator fields and the string-typed `--claim-id`; verify integration tests cover every listed position in `status`, `open`, `remove`, `add`, `claim`, and `release` without executing those commands.
- [x] 2.3 Compare the derived CLI tree's ID-valued arguments against the central registry; verify the test fails when an ID argument has no binding.
- [x] 2.4 Return empty, quiet candidate sets when read-only storage is missing, incompatible, or failing; verify tests leave state paths unchanged, invoke no hook or Git command, and preserve ordinary command errors.

## 3. Repository Input Candidates

- [x] 3.1 Complete unique registered source base names and local paths for `create --repo` and `add --repo`; verify tests cover ambiguous names, local path precedence, typed prefixes, and URL-shaped input without remote access.
- [x] 3.2 Verify Bash and `zsh` completion with repository paths and names containing spaces, quotes, and control characters; confirm safe values are preserved, unsafe control values are omitted, and shell source contains no interpolated persisted text.

## 4. Package and Release Integration

- [ ] 4.1 Add shell-discoverable loader files to the Nix package; verify the built output includes Bash and `zsh` registrations that call the installed binary.
- [ ] 4.2 Include Bash and `zsh` loader files that can be sourced in release archives; verify archive contents and registration from an extracted package.
- [ ] 4.3 Update installation and release documentation for the packaged files and shell discovery; verify the instructions in clean Bash and `zsh` sessions, including `zsh` autoload.

## 5. Cross-Cutting Validation

- [ ] 5.1 Run the relevant completion and CLI test suites plus repository checks; resolve failures and record the commands and results in `verification.md`.
- [ ] 5.2 Run `openspec validate add-shell-completions --strict` and review each shell-completion scenario against the implementation before marking this plan complete.

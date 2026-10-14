# Tasks

- [x] Add the reconciliation CLI and shared workspace selectors.
- [x] Execute checks under an exclusive operation lease with observation-safe recovery.
- [x] Add human and JSON results and shell completion bindings.
- [x] Document the behavior contract and user-facing command.
- [x] Review errors, visibility, and behavior against repository guidelines.
- [x] Run isolated ablation experiments and remove duplicate guards and tests.
- [x] Verify state updates, selectors, idempotency, claims, and operation admission.
- [x] Complete repository checks.

## Verification

- `cargo test --all-targets --all-features` passed.
- Focused reconciliation, shell completion, and workspace open tests passed on the final implementation.
- `prek run --all-files` passed, including clippy, diagnostics, formatting, and documentation checks.
- `nix flake check --no-build` passed for the host system.

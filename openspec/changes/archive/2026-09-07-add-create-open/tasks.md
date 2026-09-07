## 1. CLI and Process Handoff

- [x] 1.1 Add `--open[=<PROGRAM>]`, default it from `$SHELL`, reject invalid input and `--json` conflicts, and cover parser and resolution behavior
- [x] 1.2 Open the selected program with the workspace as its current directory, using process replacement where supported and status propagation elsewhere

## 2. Integration and Documentation

- [x] 2.1 Verify manual and automatic create hand off to explicit and default programs with the correct workspace directory
- [x] 2.2 Document option syntax, process lifecycle, claim behavior, and error handling
- [x] 2.3 Run complete Rust checks, repository hooks, strict OpenSpec validation, and Nix flake evaluation

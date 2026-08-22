## 1. Project Foundation

- [x] 1.1 Add minimal Cargo metadata, lock file, and placeholder binary; verify the package has no product behavior
- [x] 1.2 Add the thin `flake.nix` entry point and layered `nix/` modules with `flake.lock` for the Rust/Git development and build environment; verify the Flake exposes a shell and package
- [x] 1.3 Add README and `.gitignore` entries for Rust, Nix, editor, and local log artifacts; verify generated artifacts are ignored
- [x] 1.4 Add Coco-style `prek` hooks and Markdown lint configuration; verify `prek -a` passes
- [x] 1.5 Add Harper prose linting and a project dictionary; verify Markdown files pass Harper without disabling spelling checks
- [x] 1.6 Add the local Nix setup action and separate lint/build GitHub Actions jobs; verify workflow YAML and Flake entry points are valid

## 2. Boundary Verification

- [x] 2.1 Run Cargo/Flake foundation checks without adding feature implementation; verify `cargo check --locked`, `nix build`, and `nix flake check` pass in a tracked Flake checkout
- [x] 2.2 Confirm no product source modules or behavior tests are present; verify the active OpenSpec change remains `skip_specs: true`

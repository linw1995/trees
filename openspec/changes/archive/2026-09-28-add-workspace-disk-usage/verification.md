# Verification

## Functional Checks

- `cargo fmt --check` passed.
- `cargo clippy --all-targets --all-features -- -D warnings` passed.
- The full `cargo test --quiet` suite passed on macOS ARM64 with Rust 1.98.0.
  The test environment used the system Bash ahead of the Nix shell's Bash;
  the latter lacks the `complete` built-in required by existing completion
  tests. The command was:

  ```sh
  direnv exec . sh -c 'PATH=/run/current-system/sw/bin:$PATH cargo test --quiet'
  ```

- `openspec validate add-workspace-disk-usage --strict --no-interactive`
  passed.
- CLI coverage confirms that partial and unavailable disk scans preserve a
  successful status result and valid JSON. It also compares lifecycle database
  bytes, a workspace file, and a Git worktree marker before and after status.
- Observer tests cover sparse files, hard links, hidden entries, missing roots,
  arithmetic overflow, and a directory replaced by an outside symlink during
  traversal.

## Scan Cost

A temporary synthetic benchmark on macOS ARM64 used 1,024-byte files and three
warmed scans per case. The median elapsed times were:

| Case | Files | Median | Allocated bytes |
| --- | ---: | ---: | ---: |
| One workspace | 6,000 | 13.7 milliseconds | 24,576,000 |
| Sixteen workspaces | 16 × 200 | 8.0 milliseconds total | 13,107,200 |

The many-workspace path scans roots sequentially, so it has one active scan
at a time. The benchmark confirmed that an example file's contents and
modification time did not change. These are warm-cache measurements of
synthetic directories; cold filesystems and larger real workspaces can be
slower. Linux behavior is covered by the project test suite when run on Linux,
but was not measured in this local run.

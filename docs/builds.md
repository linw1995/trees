# Builds and Releases

## Release Archives

The [release workflow](../.github/workflows/CD.yaml) runs when a `v*` tag is pushed.
The tag must match the Cargo package version, for example `v0.1.0`.
After tests pass on all four platforms, it publishes Linux (`glibc` 2.35 or newer)
and macOS archives for x86_64 and ARM64 to
[GitHub Releases](https://github.com/linw1995/trees/releases).
Tags containing a prerelease suffix produce prereleases.

Each archive includes `trees`, `BUILD_INFO.txt`, `LICENSE`, and
`THIRD_PARTY_NOTICES.html`. Verify downloads against the release's `SHA256SUMS`,
extract the archive, and copy `trees` to a directory on your `PATH`.

## Build Metadata

`-V` prints the package version. `--version` also prints the embedded commit,
tracked dirty status, and UTC build timestamp. Cargo uses the current time unless
`SOURCE_DATE_EPOCH` is set; Nix and release builds use the source timestamp for
reproducibility. `GIT_COMMIT_SHA` and `GIT_DIRTY` override Git detection for builds
without Git metadata. An optional `.packaged-commit` file takes precedence over
`GIT_COMMIT_SHA`; unavailable Git metadata is reported as `unknown`.

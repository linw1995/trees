# Repository Guidelines

## Commit Messages

Commit messages must follow the [Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/) format:

```text
<type>[optional scope]: <description>
```

Use a concise, imperative English description. For example:

```text
feat(cli): add workspace status
fix: roll back failed worktree creation
docs: clarify installation instructions
```

## Pull Requests

- Pull request titles must use the same Conventional Commits format as commit messages.
- Keep pull request titles concise and written in English.
- Use `.github/pull_request_template.md` for every pull request and complete its AI Disclosure section.

## Rust Error Handling

- Define project errors as typed Snafu errors. Do not use `String` as an error channel across module or public API boundaries.
- Use `.context(...)` or `.with_context(...)` when converting a `Result` and adding error context. Make selectors visible only as broadly as the calling module requires.
- Use transparent variants and `?` when propagation should preserve the source error without adding context. Do not write `map_err` for a plain `From` conversion.
- Use `IntoError` only when wrapping a bare source error value rather than a `Result`.
- Prefer Snafu selectors, `.fail()`, and `ensure!` for source-less errors instead of constructing error variants directly.
- Preserve explicit `match` or `map_err` logic when errors must be classified, when a branch has persistence or cleanup side effects, or when recovery behavior depends on the concrete source error.
- Preserve source errors in the error chain. Do not stringify a source merely to store it in another error; text snapshots are acceptable for persisted audit details or intentionally heterogeneous multi-error reports.
- Convert errors to display text only at the outermost CLI boundary. Preserve existing user-visible messages and exit behavior when refactoring errors.

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

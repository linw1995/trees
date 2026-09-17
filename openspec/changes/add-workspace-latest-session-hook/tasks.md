## 1. Configuration and CLI

- [x] 1.1 Add typed optional hook configuration in `src/config.rs`, preserving unrelated settings. Verify missing configuration, defaults, invalid table/value types, zero timeout, relative program resolution, and rejection of an `args` setting with focused tests.
- [ ] 1.2 Add `status --no-hooks` and pass its value into report loading. Verify CLI parsing in all views and that bypass does not read even malformed hook configuration.

## 2. Protocol and Observation Model

- [x] 2.1 Add the request, response, session summary, observation, and stable issue types under the existing status module entry point. Verify version-1 serialization and a version-2 report containing independent observation timestamps.
- [x] 2.2 Implement response validation and per-workspace completion classification. Verify empty lists versus omitted results, invalid null arrays/elements, invalid later entries, empty coverage, unknown IDs, duplicate keys, invalid timestamps, missing fields, unsupported versions, trailing log text, and ignored extension fields.

## 3. Subprocess Execution

- [x] 3.1 Implement direct executable invocation without additional command-line arguments with inherited environment, configuration-directory working directory, concurrent pipe servicing, and typed Snafu errors. Verify executable paths containing spaces, no extra arguments, executable shebang scripts, one batch request, standard input EOF, and missing or non-executable files using fake providers.
- [x] 3.2 Implement total deadline, output limits, bounded standard error capture, child cleanup, and `Unix` process-group termination. Verify blocked standard input, noisy standard error, standard output overflow, nonzero exit, timeout, and descendants retaining pipe handles without hanging the test suite.

## 4. Report and Rendering Integration

- [ ] 4.1 Collect the hook observation after closing lifecycle storage and before target process observation. Verify one invocation for a nonempty inventory, no invocation for other views or failed loading, removed filtering, and no extra request entry for an excluded target.
- [ ] 4.2 Add nullable `workspace_sessions` to JSON reports while leaving workspace objects and schema version unchanged. Verify partial coverage, whole-batch failures, configuration failures, issue ordering, complete session lists in provider order, and successful exit with exactly one JSON document and no child output leakage.
- [ ] 4.3 Append the conditional human column and issue-code diagnostic line. Verify unchanged unconfigured output, first-session/empty/unavailable cells, provider order despite conflicting timestamps, full JSON lists and titles, escaped control characters, wide Unicode, 60-column truncation, and one physical line per workspace.

## 5. Documentation and Integration Validation

- [ ] 5.1 Update configuration and status documentation and add an executable protocol example. Verify the example returns multiple ordered sessions per workspace through standard input/standard output and document selection-policy ownership, standard error exposure, trust boundary, limits, and `--no-hooks`.
- [ ] 5.2 Run focused end-to-end status tests proving hook execution leaves Trees lifecycle records unchanged and preserves process reporting. Run formatting, relevant Rust tests, and repository-required checks, recording any environment limitations.
- [ ] 5.3 Validate the final specification with `openspec validate add-workspace-latest-session-hook --strict` and review implementation against all hook scenarios before marking tasks complete.

# Design

## Decisions

- Reuse the existing table and repository summary functions at the status module
  entry point. Preserve their width handling, escaping, labels, and color policy.
- Filter removed workspaces before calculating scan counts and removal candidates.
- Load repository display rows and cached workspace sizes in the read-only CLI
  preview. Consume repository rows through the existing status conversion.
- Keep presentation fields and cache decoding errors out of the core GC scan and
  execution APIs. No new candidate type, facade, or generic rendering framework
  is needed.
- Retain one CLI regression for cache wiring, repository rows, execution totals,
  and empty inventories. Existing unit tests cover size display variants, and
  lifecycle tests cover preservation and read-only scan behavior.

## Trade-Offs

The CLI reads repository metadata separately from the safety scan for eligible
workspaces. This small SQLite read avoids retaining display data in every core
candidate and cloning it during execution. Sizes retain the status view's cache
freshness; the preview does not traverse workspace files to refresh them.

`SKIPPED` counts retained candidates from the current execution. `REMOVED` counts
workspaces successfully removed by that execution. Neither includes old history.

## Review and Archive

Record isolated ablation results and the repository-guideline review in
`review.md`. Validate the final implementation, synchronize the behavior delta,
and archive only after that review passes.

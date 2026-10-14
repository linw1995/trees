# Design

Reuse the shared workspace locator with current-directory selection by default,
plus positional workspace ID and named ID, root-directory, or claim selectors.
Use writable database access and the existing reconciliation engine. Return
workspace identity, path, observed state, and changed worktree count as human
assignments or JSON.

Resolve and validate the target in a short transaction, recover expired
observation-safe operations outside transactions, then repeat target validation
and acquire a `reconcile` operation lease in one short write transaction. Observe
Git and filesystem state outside long write transactions with lease heartbeats.
Record terminal success or failure and release the operation lease. Reuse
external observation events and their existing idempotency rules.

Add `reconcile` to observation-safe access recovery so an interrupted check can
be recovered by both reconciliation and open. Structural recovery for `create`
or `add` remains the responsibility of their lifecycle commands. Do not refresh
disk usage, run status hooks, mutate Git contents, or change claims.

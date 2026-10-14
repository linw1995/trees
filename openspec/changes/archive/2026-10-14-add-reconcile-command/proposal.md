# Add Explicit Workspace Reconciliation

## Why

Persisted status can remain stale after changes made outside Trees. A dedicated
command lets callers explicitly refresh the selected workspace's observations.

## What Changes

Add
`trees reconcile` to check the current workspace or an explicitly selected
workspace and refresh lifecycle observations without changing user files or
claims.

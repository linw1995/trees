# Configure Storage

[Back to Trees](../README.md#documentation)

## Automatic Workspace Directory

Configure the automatic workspace content directory independently from the
lifecycle database:

```sh
trees config set workspaces-dir /absolute/path/to/workspaces
```

The configured value is persisted as an absolute path. If unset, Trees uses
the platform data-directory default.

## Source Clone Directory

```sh
trees config set origins-dir /path/to/origins
```

The default origin directory is `trees/origins` below the platform data
directory. `repository.origins_dir` in the configuration file overrides it;
relative values resolve against that file's directory. Each new clone occupies
`<origins-dir>/<origin-id>/<directory-name>`. Changing the setting affects only
new allocations and does not move existing sources. URL lookup can reuse a
matching source anywhere. Missing or identity-mismatched sources are excluded
from URL matching while their records remain visible; an unknown URL can
produce a new clone without changing those retained records.

See [Workspace lifecycle](workspaces.md) for source selection and allocation,
and [Cleanup](cleanup.md) for removing old workspaces.

# Design

Resolve positional IDs against workspace and origin records in one read-only
transaction. Reject IDs present in both tables. Reuse existing workspace
eligibility checks for workspace targets and return the stored source path for
origins. Close the database before starting the selected program. Do not fetch,
clone, claim, reconcile, or modify repository records.

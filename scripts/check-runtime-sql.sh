#!/usr/bin/env bash

set -euo pipefail

pattern='sql_query\(|diesel::sql!|SimpleConnection::batch_execute|\.batch_execute\('

violations="$(rg -n --glob '*.rs' --glob '!database.rs' "$pattern" src || true)"
if [[ -n "$violations" ]]; then
    printf '%s\n' "Runtime SQL usage is not allowed outside src/database.rs:" >&2
    printf '%s\n' "$violations" >&2
    exit 1
fi

allowed='batch_execute\((CONNECTION_PRAGMAS|CONNECTION_BUSY_TIMEOUT_PRAGMA|TRY_BUSY_TIMEOUT_PRAGMA)\)'
violations="$(rg -n --glob 'database.rs' "$pattern" src | rg -v "$allowed" || true)"
if [[ -n "$violations" ]]; then
    printf '%s\n' "Only the fixed SQLite connection PRAGMAs may use runtime SQL:" >&2
    printf '%s\n' "$violations" >&2
    exit 1
fi

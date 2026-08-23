#!/usr/bin/env bash

set -euo pipefail

title="${1:-${PR_TITLE:-}}"
pattern='^[a-z]+(\([[:alnum:]][[:alnum:]./_-]*\))?!?: [^[:space:]].*$'

if [[ -z "${title}" ]]; then
    printf '%s\n' "Pull request title is empty." >&2
    exit 1
fi

if [[ ! "${title}" =~ ${pattern} ]]; then
    printf 'Invalid pull request title: "%s"\n' "${title}" >&2
    printf '%s\n' 'Expected Conventional Commits format: <type>[optional scope][!]: <description>' >&2
    exit 1
fi

printf 'Valid pull request title: "%s"\n' "${title}"

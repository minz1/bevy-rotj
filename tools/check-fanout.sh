#!/usr/bin/env bash
# Usage: tools/check-fanout.sh <base-ref> <family file>, run from the repository root.
set -euo pipefail

if [[ $# -ne 2 ]]; then
    echo "usage: $0 <base-ref> crates/rotj-core/src/actor/families/<name>.rs" >&2
    exit 2
fi
base=$(git merge-base "$1" HEAD)
family=$2

if [[ ! $family =~ ^crates/rotj-core/src/actor/families/[a-z0-9_]+\.rs$ ]]; then
    echo "not a family file: $family" >&2
    exit 2
fi

changes=$( (git diff --name-status --no-renames "$base"; git status --porcelain --untracked-files=all | sed -n 's/^?? /A\t/p') | sort -u)
allowed=("crates/rotj-core/src/actor/families.rs" "docs/ACTORS.md" "$family")
failed=0

while IFS=$'\t' read -r status path; do
    [[ -z $path ]] && continue
    ok=0
    for a in "${allowed[@]}"; do
        [[ $path == "$a" ]] && ok=1
    done
    if [[ $ok -eq 0 || $status == D* ]]; then
        echo "fan-out tasks may not change: $status $path" >&2
        failed=1
    fi
done <<<"$changes"

if [[ -f $family ]] && grep -nE '#\[path|impl(<[^>]*>)? +(Body|World|Cx|OamBuilder|Slots|Schedule|Work)\b' "$family" >&2; then
    echo "the family file may not reach into core types" >&2
    failed=1
fi

exit "$failed"

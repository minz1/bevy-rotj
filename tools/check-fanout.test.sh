#!/usr/bin/env bash
# Runs tools/check-fanout.sh against throwaway repositories.
set -euo pipefail

script=$(realpath "$(dirname "$0")/check-fanout.sh")
family=crates/rotj-core/src/actor/families/k24.rs

fresh() {
    local dir
    dir=$(mktemp -d)
    git -C "$dir" init -q
    mkdir -p "$dir/crates/rotj-core/src/actor/families" "$dir/docs"
    for f in crates/rotj-core/src/actor/engine.rs crates/rotj-core/src/actor/families.rs \
        crates/rotj-core/src/actor/families/k0a.rs docs/ACTORS.md; do
        echo "// $f" >"$dir/$f"
    done
    git -C "$dir" add -A
    git -C "$dir" -c user.name=t -c user.email=t commit -qm base
    echo "$dir"
}

expect() {
    local want=$1 name=$2 dir=$3 got=0
    (cd "$dir" && "$script" HEAD "$family" 2>/dev/null) || got=$?
    rm -rf "$dir"
    if [[ $got -ne $want ]]; then
        echo "FAIL $name: exit $got, want $want"
        exit 1
    fi
    echo "ok $name"
}

d=$(fresh); echo "pub struct State;" >"$d/$family"; echo "// arm" >>"$d/crates/rotj-core/src/actor/families.rs"; echo row >>"$d/docs/ACTORS.md"
expect 0 "family file, arms and docs" "$d"

d=$(fresh); echo "// edit" >>"$d/crates/rotj-core/src/actor/engine.rs"
expect 1 "uncommitted core edit" "$d"

d=$(fresh); git -C "$d" mv crates/rotj-core/src/actor/engine.rs crates/rotj-core/src/actor/families/kff.rs
expect 1 "rename of a core file" "$d"

d=$(fresh); git -C "$d" rm -q docs/ACTORS.md
expect 1 "deletion" "$d"

d=$(fresh); echo "// edit" >>"$d/crates/rotj-core/src/actor/families/k0a.rs"
expect 1 "another family's file" "$d"

d=$(fresh); printf 'impl Body {}\n' >"$d/$family"
expect 1 "impl on a core type" "$d"

d=$(fresh); printf '#[path = "../engine.rs"]\nmod e;\n' >"$d/$family"
expect 1 "path attribute" "$d"

d=$(fresh); echo "pub struct State;" >"$d/$family"; git -C "$d" add -A; git -C "$d" -c user.name=t -c user.email=t commit -qm task; echo "// e" >>"$d/crates/rotj-core/src/actor/engine.rs"; git -C "$d" add -A; git -C "$d" -c user.name=t -c user.email=t commit -qm sneak
(cd "$d" && git tag base HEAD~2)
got=0; (cd "$d" && "$script" base "$family" 2>/dev/null) || got=$?; rm -rf "$d"
if [[ $got -ne 1 ]]; then
    echo "FAIL committed core edit"
    exit 1
fi
echo "ok committed core edit"

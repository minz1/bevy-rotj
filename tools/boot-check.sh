#!/bin/sh
# Pre-tag check: the port must boot *and keep running*. Every release tag is
# expected to pass this (the CI `boot` job runs the same check under xvfb).
#
#   tools/boot-check.sh [seconds]
#
# The oracle is the exit status: `timeout` returns 124 when it had to kill the
# app, which is the success case here — it means the app was still running when
# the time ran out. An earlier version of this script only looked for a window
# and for the absence of a panic, which let a genuinely broken build pass (the
# event loop died 1.7s in, the window had already been created).
set -eu

SECONDS_TO_RUN="${1:-20}"
LOG="$(mktemp)"
trap 'rm -f "$LOG"' EXIT

# Build first: a cold rebuild can easily outlast the boot budget, which would
# look like "never created a window" (it did once).
echo "building..."
cargo build -p batman-rotj >/dev/null 2>&1 || { echo "FAIL: the workspace does not build" >&2; exit 1; }

fail() {
    echo "FAIL: $1" >&2
    tail -30 "$LOG" >&2
    exit 1
}

echo "checking the message without a ROM..."
set +e
ROTJ_ROM=/nonexistent/rom.nes timeout 10 target/debug/batman-rotj >"$LOG" 2>&1 </dev/null
STATUS=$?
set -e
if [ "$STATUS" -ne 2 ] || ! grep -q "No usable Batman" "$LOG"; then
    fail "without a ROM the app must exit 2 with its message (status $STATUS)"
fi

if [ -z "${ROTJ_ROM:-}" ] && [ ! -f "raw/Batman - Return of the Joker (USA).nes" ]; then
    [ -z "${ROTJ_REQUIRE_ROM:-}" ] || fail "ROTJ_REQUIRE_ROM is set but there is no ROM (ROTJ_ROM or raw/)"
    echo "OK: no ROM, so only the no-ROM message was checked"
    exit 0
fi

echo "running the port for ${SECONDS_TO_RUN}s..."
set +e
timeout "$SECONDS_TO_RUN" target/debug/batman-rotj >"$LOG" 2>&1
STATUS=$?
set -e

if grep -qi "panicked" "$LOG"; then
    fail "the app panicked"
fi
if ! grep -q "Creating new window" "$LOG"; then
    fail "the app never created a window"
fi
if [ "$STATUS" -ne 124 ]; then
    if grep -qi "Exit Failure" "$LOG"; then
        fail "the event loop died (exit status $STATUS)"
    fi
    fail "the app exited on its own (status $STATUS)"
fi

echo "OK: still running after ${SECONDS_TO_RUN}s (window created, no panic, event loop alive)"

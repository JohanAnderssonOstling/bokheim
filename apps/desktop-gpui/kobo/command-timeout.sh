#!/bin/sh
# Run a noninteractive firmware command with a bounded lifetime.
set -u
SECONDS_LIMIT=$1
shift
case "$SECONDS_LIMIT" in ''|*[!0-9]*) exit 2 ;; esac
[ "$SECONDS_LIMIT" -gt 0 ] && [ "$#" -gt 0 ] || exit 2

"$@" &
COMMAND_PID=$!
(
    sleep "$SECONDS_LIMIT"
    kill -KILL "$COMMAND_PID" 2>/dev/null || true
) </dev/null >/dev/null 2>&1 &
WATCHDOG_PID=$!

cleanup() {
    kill -TERM "$WATCHDOG_PID" 2>/dev/null || true
    wait "$WATCHDOG_PID" 2>/dev/null || true
}
interrupted() {
    kill -TERM "$COMMAND_PID" 2>/dev/null || true
    exit 143
}
trap cleanup 0
trap interrupted 1 2 15
wait "$COMMAND_PID"
STATUS=$?
exit "$STATUS"

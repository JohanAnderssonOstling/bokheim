#!/bin/sh
# Run a noninteractive firmware command with a bounded lifetime.
set -u
SECONDS_LIMIT=$1
shift
case "$SECONDS_LIMIT" in ''|*[!0-9]*) exit 2 ;; esac
[ "$SECONDS_LIMIT" -gt 0 ] && [ "$#" -gt 0 ] || exit 2

# The app supplies setsid itself; not every firmware ships a setsid utility.
# exec preserves this PID as the process-group ID for the actual command.
HELPER_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
"$HELPER_DIR/desktop-gpui-kobo" --run-isolated "$@" &
COMMAND_PID=$!
(
    TIMER_PID=''
    stop_timer() {
        if [ -n "$TIMER_PID" ]; then
            kill -KILL "$TIMER_PID" 2>/dev/null || true
            wait "$TIMER_PID" 2>/dev/null || true
        fi
        exit 0
    }
    trap stop_timer 1 2 15
    sleep "$SECONDS_LIMIT" &
    TIMER_PID=$!
    wait "$TIMER_PID"
    kill -KILL "-$COMMAND_PID" 2>/dev/null || true
    kill -KILL "$COMMAND_PID" 2>/dev/null || true
) </dev/null >/dev/null 2>&1 &
WATCHDOG_PID=$!

cleanup() {
    kill -TERM "$WATCHDOG_PID" 2>/dev/null || true
    wait "$WATCHDOG_PID" 2>/dev/null || true
}
interrupted() {
    kill -KILL "-$COMMAND_PID" 2>/dev/null || true
    kill -KILL "$COMMAND_PID" 2>/dev/null || true
    wait "$COMMAND_PID" 2>/dev/null || true
    exit 143
}
trap cleanup 0
trap interrupted 1 2 15
wait "$COMMAND_PID"
STATUS=$?
# A command which exits early must not leave worker processes behind either.
kill -KILL "-$COMMAND_PID" 2>/dev/null || true
exit "$STATUS"

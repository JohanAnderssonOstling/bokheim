#!/bin/sh
# Installed on the internal filesystem so SSH also works while onboard storage
# is exported over USB. BusyBox init restarts this supervisor if it exits.
set -u
STATE=/etc/bokheim-ssh
PID_FILE=/var/run/bokheim-ssh.pid
ONBOARD_SERVER=/mnt/onboard/.adds/bokheim/ssh/dropbear
export PATH=/sbin:/bin:/usr/sbin:/usr/bin
unset LD_LIBRARY_PATH LD_PRELOAD
umask 077

exec </dev/null >>"$STATE/service.log" 2>&1

running() {
    [ -r "$PID_FILE" ] || return 1
    server_pid=$(cat "$PID_FILE")
    case "$server_pid" in ''|*[!0-9]*) return 1 ;; esac
    server_exe=$(readlink "/proc/$server_pid/exe" 2>/dev/null) || return 1
    case "$server_exe" in
        "$STATE/dropbear"|"$ONBOARD_SERVER"|"$STATE/dropbear (deleted)"|"$ONBOARD_SERVER (deleted)") ;;
        *) return 1 ;;
    esac
    kill -0 "$server_pid" 2>/dev/null
}

rotate_service_log() {
    if [ -f "$STATE/service.log" ] && [ "$(wc -c <"$STATE/service.log")" -ge 1048576 ]; then
        mv -f "$STATE/service.log" "$STATE/service.previous.log"
        exec >>"$STATE/service.log" 2>&1
    fi
}

rotate_service_log
printf '[%s] SSH supervisor started\n' "$(date)"
while :; do
    # Preserve an existing launcher-started server and its connections. Once it
    # exits, this service takes over using its internal-filesystem executable.
    if running; then
        sleep 5
        continue
    fi
    if [ ! -x "$STATE/dropbear" ] || [ ! -s "$STATE/hostkey" ] || [ ! -s "$STATE/authorized_keys" ]; then
        sleep 30
        continue
    fi
    if [ -f "$STATE/server.log" ] && [ "$(wc -c <"$STATE/server.log")" -ge 1048576 ]; then
        mv -f "$STATE/server.log" "$STATE/server.previous.log"
    fi
    rotate_service_log
    printf '[%s] Starting supervised SSH on port 2222\n' "$(date)"
    "$STATE/dropbear" -F -r "$STATE/hostkey" -D "$STATE" \
        -p 2222 -P "$PID_FILE" -j -k -W 1048576 -K 30 -I 600 \
        >>"$STATE/server.log" 2>&1
    status=$?
    printf '[%s] SSH exited: status=%s; restarting in 5 seconds\n' "$(date)" "$status"
    sleep 5
done

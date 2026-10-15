#!/bin/sh
# Optional developer SSH server. Authentication stays on the internal ext filesystem,
# since the onboard FAT filesystem cannot enforce authorized_keys permissions.
set -eu
SSH_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
STATE=/etc/bokheim-ssh
PID_FILE=/var/run/bokheim-ssh.pid
umask 077

[ "$(id -u)" = 0 ] || { printf 'Kobo SSH must start as root\n' >&2; exit 1; }
[ ! -L "$STATE" ] || { printf 'Kobo SSH state directory is a symlink\n' >&2; exit 1; }
mkdir -p "$STATE"
chmod 700 "$STATE"

# This directory belongs only to Bokheim; preserve existing system SSH settings.
if [ -f "$SSH_DIR/authorized_keys" ]; then
    [ ! -L "$STATE/authorized_keys" ] && [ ! -L "$STATE/authorized_keys.new" ] || exit 1
    cat "$SSH_DIR/authorized_keys" > "$STATE/authorized_keys.new"
    chmod 600 "$STATE/authorized_keys.new"
    mv -f "$STATE/authorized_keys.new" "$STATE/authorized_keys"
fi
[ -s "$STATE/authorized_keys" ] || { printf 'No Kobo SSH public key configured\n' >&2; exit 1; }
[ ! -L "$STATE/hostkey" ] || exit 1
if [ ! -s "$STATE/hostkey" ]; then
    if [ -s "$SSH_DIR/hostkey-seed" ]; then
        [ ! -L "$STATE/hostkey.new" ] || exit 1
        cat "$SSH_DIR/hostkey-seed" > "$STATE/hostkey.new"
        chmod 600 "$STATE/hostkey.new"
        mv -f "$STATE/hostkey.new" "$STATE/hostkey"
    else
        LD_LIBRARY_PATH= "$SSH_DIR/dropbearkey" -t ed25519 -f "$STATE/hostkey"
    fi
fi
rm -f "$SSH_DIR/hostkey-seed"
chmod 600 "$STATE/hostkey" "$STATE/authorized_keys"

running() {
    [ -r "$PID_FILE" ] || return 1
    server_pid=$(cat "$PID_FILE")
    case "$server_pid" in ''|*[!0-9]*) return 1 ;; esac
    server_exe=$(readlink "/proc/$server_pid/exe" 2>/dev/null) || return 1
    case "$server_exe" in
        "$SSH_DIR/dropbear"|"$STATE/dropbear"|"$SSH_DIR/dropbear (deleted)"|"$STATE/dropbear (deleted)") ;;
        *) return 1 ;;
    esac
    kill -0 "$server_pid" 2>/dev/null
}

if ! running; then
    # Password authentication is disabled at build time. Restrict forwarding too.
    chmod +x "$SSH_DIR/dropbear" "$SSH_DIR/dropbearkey"
    (
        trap '' HUP
        exec env LD_LIBRARY_PATH= "$SSH_DIR/dropbear" -F -r "$STATE/hostkey" -D "$STATE" \
            -p 2222 -P "$PID_FILE" -j -k -W 1048576 -K 30 -I 600
    ) </dev/null >>"$STATE/server.log" 2>&1 &
    remaining=20
    until running; do
        remaining=$((remaining-1))
        [ "$remaining" -gt 0 ] || { printf 'Kobo SSH failed to start\n' >&2; exit 1; }
        sleep 0.1
    done
fi
printf 'Kobo SSH server running on port 2222 (public key only)\n'
ifconfig 2>/dev/null | grep 'inet ' || true

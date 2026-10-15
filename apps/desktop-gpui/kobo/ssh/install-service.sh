#!/bin/sh
# Explicit developer opt-in. Run on the Kobo after inspecting its init setup;
# the normal application installer never changes firmware boot configuration.
set -eu
SSH_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
STATE=/etc/bokheim-ssh
ENTRY='::respawn:/bin/sh /etc/bokheim-ssh/supervise.sh'
umask 077

[ "$(id -u)" = 0 ] || { printf 'Install the SSH service as root\n' >&2; exit 1; }
case "$(readlink /proc/1/exe)" in
    */busybox) ;;
    *) printf 'Unsupported init; refusing to change boot configuration\n' >&2; exit 1 ;;
esac
[ -f /etc/inittab ] && [ ! -L /etc/inittab ] || { printf 'Expected a regular /etc/inittab\n' >&2; exit 1; }
grep -Eq '^[^#]*:sysinit:' /etc/inittab || { printf 'No BusyBox sysinit entry; refusing update\n' >&2; exit 1; }
[ ! -L "$STATE" ] || { printf 'SSH state directory is a symlink\n' >&2; exit 1; }
[ -s "$SSH_DIR/dropbear" ] && [ -s "$SSH_DIR/supervise.sh" ] || { printf 'SSH service payload is incomplete\n' >&2; exit 1; }
/bin/sh -n "$SSH_DIR/supervise.sh"

existing=$(grep -Fc '/etc/bokheim-ssh/supervise.sh' /etc/inittab || true)
case "$existing" in
    0) ;;
    1) grep -Fxq "$ENTRY" /etc/inittab || { printf 'Conflicting SSH boot entry\n' >&2; exit 1; } ;;
    *) printf 'Duplicate SSH boot entries; refusing update\n' >&2; exit 1 ;;
esac

# Provision the existing public key and host identity and preserve the current
# server. No reboot or termination of the deployment connection is needed.
/bin/sh "$SSH_DIR/start.sh"
for name in dropbear supervise.sh inittab.before-service; do
    [ ! -L "$STATE/$name" ] || { printf 'Unexpected SSH service symlink\n' >&2; exit 1; }
done
available_kib=$(df -Pk "$STATE" | awk 'NR==2 {print $4}')
required_kib=$((($(wc -c <"$SSH_DIR/dropbear") + 1023) / 1024 + 1024))
case "$available_kib" in ''|*[!0-9]*) printf 'Could not read internal free space\n' >&2; exit 1 ;; esac
[ "$available_kib" -ge "$required_kib" ] || { printf 'Insufficient internal space for SSH service\n' >&2; exit 1; }

binary_tmp=''
supervisor_tmp=''
inittab_tmp=''
cleanup() {
    [ -z "$binary_tmp" ] || rm -f "$binary_tmp"
    [ -z "$supervisor_tmp" ] || rm -f "$supervisor_tmp"
    [ -z "$inittab_tmp" ] || rm -f "$inittab_tmp"
}
trap cleanup 0
trap 'exit 1' 1 2 15

binary_tmp=$(mktemp "$STATE/dropbear.XXXXXX")
supervisor_tmp=$(mktemp "$STATE/supervise.XXXXXX")
cp "$SSH_DIR/dropbear" "$binary_tmp"
cp "$SSH_DIR/supervise.sh" "$supervisor_tmp"
chmod 700 "$binary_tmp" "$supervisor_tmp"
cmp "$SSH_DIR/dropbear" "$binary_tmp"
cmp "$SSH_DIR/supervise.sh" "$supervisor_tmp"

# Keep the first pre-service configuration, and append only our single action.
# Atomically rename each complete file; never truncate the live boot file.
if [ ! -e "$STATE/inittab.before-service" ]; then
    cp -p /etc/inittab "$STATE/inittab.before-service"
fi
inittab_tmp=$(mktemp /etc/inittab.bokheim.XXXXXX)
cp -p /etc/inittab "$inittab_tmp"
if [ "$existing" = 0 ]; then
    printf '\n%s\n' "$ENTRY" >>"$inittab_tmp"
fi
[ "$(grep -Fxc "$ENTRY" "$inittab_tmp")" = 1 ]
# Flush complete staged files before making the new boot action visible.
sync
mv -f "$binary_tmp" "$STATE/dropbear"
binary_tmp=''
mv -f "$supervisor_tmp" "$STATE/supervise.sh"
supervisor_tmp=''
mv -f "$inittab_tmp" /etc/inittab
inittab_tmp=''
sync

# BusyBox rereads inittab on HUP and starts new respawn actions immediately.
kill -HUP 1
printf 'Installed persistent SSH service on port 2222 (public key only).\n'
printf 'Boot configuration backup: %s/inittab.before-service\n' "$STATE"
printf 'Supervisor log: %s/service.log\n' "$STATE"

#!/bin/sh
# Requires an existing SSH server, authorized key, and trusted host key on the Kobo.
set -eu

if [ "$#" -lt 1 ] || [ "$#" -gt 2 ]; then
    printf 'Usage: %s root@KOBO_IP [package.tar.gz]\nSet BOKHEIM_KOBO_SSH_PORT and optionally BOKHEIM_KOBO_SSH_KEY for the server.\n' "$0" >&2
    exit 2
fi
KOBO_HOST=$1
case "$KOBO_HOST" in -*) printf 'Invalid SSH destination\n' >&2; exit 2 ;; esac
KOBO_PORT=${BOKHEIM_KOBO_SSH_PORT:-22}
case "$KOBO_PORT" in ''|*[!0-9]*) printf 'Invalid SSH port\n' >&2; exit 2 ;; esac
[ "$KOBO_PORT" -ge 1 ] && [ "$KOBO_PORT" -le 65535 ] || exit 2
SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
ARCHIVE=${2:-${BOKHEIM_KOBO_ARCHIVE:-}}
[ -n "$ARCHIVE" ] || ARCHIVE=$("$SCRIPT_DIR/package-kobo.sh")
LOCAL_STAGE=$(mktemp -d)
REMOTE_STAGE=''

ssh_kobo() {
    if [ -n "${BOKHEIM_KOBO_SSH_KEY:-}" ]; then
        ssh -i "$BOKHEIM_KOBO_SSH_KEY" -o IdentitiesOnly=yes -p "$KOBO_PORT" \
            -o BatchMode=yes -o ConnectTimeout=10 -o ServerAliveInterval=5 \
            -o ServerAliveCountMax=3 "$KOBO_HOST" "$@"
        return
    fi
    ssh -p "$KOBO_PORT" -o BatchMode=yes -o ConnectTimeout=10 \
        -o ServerAliveInterval=5 -o ServerAliveCountMax=3 "$KOBO_HOST" "$@"
}

cleanup() {
    if [ -n "$REMOTE_STAGE" ]; then
        ssh_kobo "rm -f '$REMOTE_STAGE/update.tar.gz' '$REMOTE_STAGE/checksums' '$REMOTE_STAGE/nickelmenu' '$REMOTE_STAGE/bokheim.png' '$REMOTE_STAGE/bokheim-active.png'; rmdir '$REMOTE_STAGE'" </dev/null >/dev/null 2>&1 || true
    fi
    rm -f "$LOCAL_STAGE/checksums"
    rmdir "$LOCAL_STAGE" || true
}
trap cleanup 0

python3 "$SCRIPT_DIR/verify-kobo-package.py" "$ARCHIVE" --manifest "$LOCAL_STAGE/checksums"
ARCHIVE_SHA=$(python3 - "$ARCHIVE" <<'PY'
import hashlib, sys
with open(sys.argv[1], 'rb') as archive:
    print(hashlib.file_digest(archive, 'sha256').hexdigest())
PY
)
REMOTE_STAGE=$(ssh_kobo 'set -eu
    test -d /mnt/onboard/.kobo
    test -f /usr/local/Kobo/imageformats/libnm.so || { echo "Install NickelMenu over USB first" >&2; exit 1; }
    command -v sha256sum >/dev/null
    for comm in /proc/[0-9]*/comm; do
        test -r "$comm" || continue
        case "$(cat "$comm")" in desktop-gpui-ko|desktop-gpui-kobo) echo "Quit Bokheim before Wi-Fi deployment" >&2; exit 1 ;; esac
    done
    mktemp -d /mnt/onboard/.adds/bokheim-update.XXXXXX')
case "$REMOTE_STAGE" in /mnt/onboard/.adds/bokheim-update.*) suffix=${REMOTE_STAGE##*.} ;; *) REMOTE_STAGE=''; printf 'Invalid remote staging path\n' >&2; exit 1 ;; esac
case "$suffix" in ''|*[!a-zA-Z0-9]*) REMOTE_STAGE=''; printf 'Invalid remote staging path\n' >&2; exit 1 ;; esac

# Streaming over SSH works with Dropbear without an SFTP server or remote scp.
ssh_kobo "cat > '$REMOTE_STAGE/update.tar.gz'" <"$ARCHIVE"
ssh_kobo "cat > '$REMOTE_STAGE/checksums'" <"$LOCAL_STAGE/checksums"
ssh_kobo "cat > '$REMOTE_STAGE/nickelmenu'" <"$SCRIPT_DIR/../kobo/nickelmenu"
ssh_kobo "cat > '$REMOTE_STAGE/bokheim.png'" <"$SCRIPT_DIR/../../ui/design-tokens/assets/icon/bokheim-kobo.png"
ssh_kobo "cat > '$REMOTE_STAGE/bokheim-active.png'" <"$SCRIPT_DIR/../../ui/design-tokens/assets/icon/bokheim-kobo-active.png"
ssh_kobo "set -eu
    printf '%s  %s\\n' '$ARCHIVE_SHA' '$REMOTE_STAGE/update.tar.gz' | sha256sum -c -
    for comm in /proc/[0-9]*/comm; do
        test -r \"\$comm\" || continue
        case \"\$(cat \"\$comm\")\" in desktop-gpui-ko|desktop-gpui-kobo) echo 'Quit Bokheim before Wi-Fi deployment' >&2; exit 1 ;; esac
    done
    tar -xzf '$REMOTE_STAGE/update.tar.gz' -C /mnt/onboard
    cd /mnt/onboard
    sha256sum -c '$REMOTE_STAGE/checksums'
    mkdir -p .adds/bokheim/nickelmenu-icons
    cp '$REMOTE_STAGE/bokheim.png' .adds/bokheim/nickelmenu-icons/bokheim.png
    cp '$REMOTE_STAGE/bokheim-active.png' .adds/bokheim/nickelmenu-icons/bokheim-active.png
    cmp '$REMOTE_STAGE/bokheim.png' .adds/bokheim/nickelmenu-icons/bokheim.png
    cmp '$REMOTE_STAGE/bokheim-active.png' .adds/bokheim/nickelmenu-icons/bokheim-active.png
    cp '$REMOTE_STAGE/nickelmenu' .adds/nm/desktop-gpui-kobo
    cmp '$REMOTE_STAGE/nickelmenu' .adds/nm/desktop-gpui-kobo
    sync"
printf 'Installed and verified Bokheim over Wi-Fi. Tap "Bokheim" on the reader.\n'

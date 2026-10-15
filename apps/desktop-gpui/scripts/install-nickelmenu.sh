#!/bin/sh
set -eu
CHECK_ONLY=0
if [ "${1:-}" = --check ]; then CHECK_ONLY=1; shift; fi
[ "$#" -eq 1 ] || { printf 'Usage: %s [--check] /path/to/KOBOeReader\n' "$0" >&2; exit 2; }
DEVICE_ROOT=${1%/}
SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
PAYLOAD="$SCRIPT_DIR/../kobo/nickelmenu-install/KoboRoot.tgz"
SHA256=c65077192df4c0857b0ff3704d708053355b311ce2497721c6d29ab5526199d2
[ -d "$DEVICE_ROOT/.kobo" ] || { printf 'Kobo marker directory is missing\n' >&2; exit 1; }
actual_sha=$(python3 - "$PAYLOAD" <<'PY'
import hashlib, sys
with open(sys.argv[1], 'rb') as payload:
    print(hashlib.file_digest(payload, 'sha256').hexdigest())
PY
)
[ "$actual_sha" = "$SHA256" ] || { printf 'NickelMenu checksum mismatch\n' >&2; exit 1; }
firmware=$(awk -F, '{ print $3 }' "$DEVICE_ROOT/.kobo/version")
case "$firmware" in
    4.*) minor=$(printf '%s\n' "$firmware" | cut -d . -f 2); [ "$minor" -ge 6 ] ;;
    *) printf 'NickelMenu requires Kobo firmware 4.6 or later in the 4.x series\n' >&2; exit 1 ;;
esac
if [ -f "$DEVICE_ROOT/.kobo/KoboRoot.tgz" ] && ! cmp -s "$PAYLOAD" "$DEVICE_ROOT/.kobo/KoboRoot.tgz"; then
    printf 'A different Kobo firmware/update package is pending. Finish that update first; it will not be overwritten.\n' >&2
    exit 1
fi
if [ -d "$DEVICE_ROOT/.kobo/upgrade" ] && [ -n "$(ls -A "$DEVICE_ROOT/.kobo/upgrade")" ]; then
    printf 'A Kobo firmware upgrade is pending. Finish that upgrade first.\n' >&2
    exit 1
fi
[ "$CHECK_ONLY" -eq 0 ] || exit 0
ICONS="$DEVICE_ROOT/.adds/bokheim/nickelmenu-icons"
mkdir -p "$ICONS"
cp "$SCRIPT_DIR/../../ui/design-tokens/assets/icon/bokheim-kobo.png" "$ICONS/bokheim.png"
cp "$SCRIPT_DIR/../../ui/design-tokens/assets/icon/bokheim-kobo-active.png" "$ICONS/bokheim-active.png"
cmp "$SCRIPT_DIR/../../ui/design-tokens/assets/icon/bokheim-kobo.png" "$ICONS/bokheim.png"
cmp "$SCRIPT_DIR/../../ui/design-tokens/assets/icon/bokheim-kobo-active.png" "$ICONS/bokheim-active.png"
TEMPORARY=$(mktemp "$DEVICE_ROOT/.kobo/.bokheim-nickelmenu.XXXXXX")
trap '[ -z "$TEMPORARY" ] || rm -f -- "$TEMPORARY"' 0
cp "$PAYLOAD" "$TEMPORARY"
cmp -s "$PAYLOAD" "$TEMPORARY"
mv "$TEMPORARY" "$DEVICE_ROOT/.kobo/KoboRoot.tgz"
TEMPORARY=''
printf 'Bokheim direct-launch integration queued for installation when the Kobo disconnects.\n'

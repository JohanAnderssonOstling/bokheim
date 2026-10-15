#!/bin/sh
set -eu

if [ "$#" -ne 1 ]; then
    printf 'Usage: %s /path/to/KOBOeReader\n' "$0" >&2
    exit 2
fi

DEVICE_ROOT=${1%/}
if [ ! -d "$DEVICE_ROOT/.kobo" ]; then
    printf '%s does not look like a mounted Kobo filesystem (.kobo is missing)\n' "$DEVICE_ROOT" >&2
    exit 1
fi

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
ARCHIVE=${BOKHEIM_KOBO_ARCHIVE:-}
[ -n "$ARCHIVE" ] || ARCHIVE=$("$SCRIPT_DIR/package-kobo.sh")
sh "$SCRIPT_DIR/install-nickelmenu.sh" --check "$DEVICE_ROOT"
python3 "$SCRIPT_DIR/verify-kobo-package.py" "$ARCHIVE"
tar --no-same-owner -xzf "$ARCHIVE" -C "$DEVICE_ROOT"
python3 "$SCRIPT_DIR/verify-kobo-package.py" "$ARCHIVE" --installed "$DEVICE_ROOT"
cp "$SCRIPT_DIR/../kobo/nickelmenu" "$DEVICE_ROOT/.adds/nm/desktop-gpui-kobo"
cmp "$SCRIPT_DIR/../kobo/nickelmenu" "$DEVICE_ROOT/.adds/nm/desktop-gpui-kobo"
sh "$SCRIPT_DIR/install-nickelmenu.sh" "$DEVICE_ROOT"
sync

printf 'Installed Bokheim under %s/.adds/bokheim\n' "$DEVICE_ROOT"
if sh "$SCRIPT_DIR/eject-kobo.sh" "$DEVICE_ROOT"; then
    printf 'Let the Kobo restart, then tap "Bokheim" to open the app.\n'
else
    printf 'Installation succeeded, but automatic eject failed. Safely eject the Kobo manually, then let it restart.\n' >&2
fi

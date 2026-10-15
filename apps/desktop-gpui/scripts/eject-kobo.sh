#!/bin/sh
set -eu
[ "$#" -eq 1 ] || { printf 'Usage: %s /path/to/KOBOeReader\n' "$0" >&2; exit 2; }
DEVICE_ROOT=${1%/}
case "$(uname -s)" in
    Linux)
        device=$(findmnt --noheadings --output SOURCE --mountpoint "$DEVICE_ROOT")
        case "$device" in /dev/*) ;; *) printf 'Kobo path is not a mounted block device\n' >&2; exit 1 ;; esac
        sync --file-system "$DEVICE_ROOT"
        udisksctl unmount --block-device "$device" --no-user-interaction
        if udisksctl power-off --block-device "$device" --no-user-interaction; then
            printf 'Kobo safely ejected.\n'
        else
            printf 'Kobo safely unmounted. Disconnect its USB cable.\n'
        fi
        ;;
    Darwin) diskutil eject "$DEVICE_ROOT" ;;
    *) printf 'Automatic Kobo eject is unavailable on this host\n' >&2; exit 1 ;;
esac

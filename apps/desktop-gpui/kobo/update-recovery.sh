#!/bin/sh
# Runs with firmware libc, even if the application's runtime was partly replaced.
set -eu

APP=${1:-/mnt/onboard/.adds/bokheim}
case "$APP" in */.adds/bokheim) ROOT=${APP%/.adds/bokheim} ;; *) exit 1 ;; esac
STATE="$APP/.update"
[ -f "$STATE/installing" ] || exit 0
[ -f "$STATE/files" ] && [ -f "$STATE/previous" ] || {
    printf 'The update recovery journal is incomplete. Reinstall Bokheim over USB.\n' >&2
    exit 1
}

validate_relative() {
    case "$1" in
        ''|*[!a-zA-Z0-9_./-]*|../*|*/../*|*/..|/*) exit 1 ;;
        .adds/bokheim/.update/*) exit 1 ;;
        .adds/bokheim/*|.adds/nm/desktop-gpui-kobo) ;;
        *) exit 1 ;;
    esac
}

# Free a failed partial copy before restoring anything, even when it belongs
# to a later file in the journal. Rollback must work after a disk-full error.
while IFS= read -r relative; do
    validate_relative "$relative"
    rm -f "$ROOT/$relative.bokheim-update-tmp" "$ROOT/$relative.bokheim-recovery-tmp"
done < "$STATE/files"

while IFS= read -r relative; do
    validate_relative "$relative"
    target="$ROOT/$relative"
    if grep -Fxq "$relative" "$STATE/previous"; then
        backup="$STATE/backup/$relative"
        [ -f "$backup" ] || { printf 'Missing update backup: %s\n' "$relative" >&2; exit 1; }
        cp "$backup" "$target.bokheim-recovery-tmp"
        mv -f "$target.bokheim-recovery-tmp" "$target"
    else
        rm -f "$target"
    fi
    rm -f "$target.bokheim-update-tmp"
done < "$STATE/files"
sync
rm -f "$STATE/ready.json" "$STATE/installing"
printf 'The interrupted update was rolled back. Download it again and retry.\n' >"$STATE/error"
sync
printf 'Restored Bokheim after an interrupted update. Books and settings were kept.\n'

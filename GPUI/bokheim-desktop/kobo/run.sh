#!/bin/sh

WORKDIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
LOG="$WORKDIR/bokheim-kobo.log"
export BOKHEIM_KOBO_CANVAS_MODE_FILE="${BOKHEIM_KOBO_CANVAS_MODE_FILE:-$WORKDIR/data/canvas-mode}"
export BOKHEIM_WESTERN_SHAPING_BENCHMARK=1
NICKEL_WAS_RUNNING=0

if pidof nickel >/dev/null 2>&1; then
    NICKEL_WAS_RUNNING=1
fi

restart_nickel() {
    if [ "$NICKEL_WAS_RUNNING" -ne 1 ] || pidof nickel >/dev/null 2>&1; then
        return
    fi
    (
        cd /
        export LD_LIBRARY_PATH=/usr/local/Kobo
        /usr/local/Kobo/hindenburg >/dev/null 2>&1 &
        LIBC_FATAL_STDERR_=1 /usr/local/Kobo/nickel \
            -platform kobo -skipFontLoad >/dev/null 2>&1 &
        if command -v udevadm >/dev/null 2>&1; then
            udevadm trigger >/dev/null 2>&1 &
        fi
    )
}

trap restart_nickel 0 1 2 15

{
    printf '\n[%s] starting Bokheim Kobo\n' "$(date)"
    cd "$WORKDIR" || exit 1
    chmod +x bokheim-kobo fbink

    LOADING_TEXT='Bokheim Kobo
Loading...'
    if ! "$WORKDIR/fbink" -q -c -f -p -m -M -W GC16 -S 3 "$LOADING_TEXT"; then
        printf '[%s] Could not present Bokheim loading screen; continuing startup\n' "$(date)"
    fi

    if [ "$NICKEL_WAS_RUNNING" -eq 1 ]; then
        sync
        killall -TERM nickel hindenburg sickel fickel adobehost foxitpdf iink \
            >/dev/null 2>&1 || true
        sleep 2
    fi

    if ! "$WORKDIR/bokheim-kobo"; then
        "$WORKDIR/fbink" -q -c -f -p -m -M -h \
            "Bokheim Kobo failed; see bokheim-kobo.log"
        sleep 10
        exit 1
    fi

    printf '[%s] Bokheim Kobo complete\n' "$(date)"
} >>"$LOG" 2>&1

exit 0

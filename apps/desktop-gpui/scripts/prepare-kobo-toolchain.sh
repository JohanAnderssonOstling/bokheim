#!/bin/sh
# PDFium's ARM runtime needs glibc. Keep the toolchain and FBInk build separate
# from GPUI's static musl demo and bundle this libc with the application.
set -eu

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
REPO_ROOT=$(CDPATH= cd -- "$SCRIPT_DIR/../../.." && pwd)
GPUI_FORK=$(CDPATH= cd -- "$REPO_ROOT/../GPUI-Fork" && pwd)
OUTPUT="$REPO_ROOT/target/desktop-gpui-kobo"
NAME=armv7-eabihf--glibc--stable-2024.05-1
SHA256=608263bc9dc3eadf0962ddb1165f1c2291001190f9927dee47d464e26374462c
TOOLCHAIN="$OUTPUT/toolchain/$NAME"
CROSS="$TOOLCHAIN/bin/arm-buildroot-linux-gnueabihf"

if [ ! -x "$CROSS-gcc" ]; then
    mkdir -p "$OUTPUT/toolchain"
    ARCHIVE="$OUTPUT/toolchain/$NAME.tar.xz"
    if [ ! -f "$ARCHIVE" ]; then
        curl --fail --location --silent --show-error --retry 3 \
            "https://toolchains.bootlin.com/downloads/releases/toolchains/armv7-eabihf/tarballs/$NAME.tar.xz" \
            -o "$ARCHIVE.partial"
        mv "$ARCHIVE.partial" "$ARCHIVE"
    fi
    if [ "$(sha256sum "$ARCHIVE" | cut -d ' ' -f 1)" != "$SHA256" ]; then
        printf 'Kobo glibc toolchain checksum mismatch\n' >&2
        exit 1
    fi
    tar -xJf "$ARCHIVE" -C "$OUTPUT/toolchain"
fi

FBINK_ARCHIVE="$GPUI_FORK/target/gpui-kobo/FBInk-v1.25.0.tar.xz"
if [ "$(sha256sum "$FBINK_ARCHIVE" | cut -d ' ' -f 1)" != d598e99ed20994e08e1bb768512bc55c97cee941ffe6b1ac237aa754690e1ed5 ]; then
    printf 'FBInk source checksum mismatch\n' >&2
    exit 1
fi
FBINK_ROOT="$OUTPUT/fbink-build/FBInk-v1.25.0"
FLAGS=${KOBO_CFLAGS:--O3}
BUILD_KEY="glibc $NAME MINIMAL=1 BITMAP=1 IMAGE=1 $FLAGS"
if [ ! -f "$FBINK_ROOT/Release/libgpui_fbink_shim.a" ] || \
    [ "$(cat "$OUTPUT/fbink-build/build-config" 2>/dev/null || true)" != "$BUILD_KEY" ]; then
    mkdir -p "$OUTPUT/fbink-build"
    tar -xJf "$FBINK_ARCHIVE" -C "$OUTPUT/fbink-build"
    make -C "$FBINK_ROOT" distclean >/dev/null
    make -C "$FBINK_ROOT" -j "${CARGO_BUILD_JOBS:-3}" staticlib \
        MINIMAL=1 BITMAP=1 IMAGE=1 CROSS_TC="$CROSS" CFLAGS="$FLAGS" >&2
    "$CROSS-gcc" $FLAGS -std=c11 -I"$FBINK_ROOT" \
        -c "$GPUI_FORK/crates/gpui_kobo/native/fbink_shim.c" \
        -o "$FBINK_ROOT/Release/gpui_fbink_shim.o"
    "$CROSS-ar" rcs "$FBINK_ROOT/Release/libgpui_fbink_shim.a" \
        "$FBINK_ROOT/Release/gpui_fbink_shim.o"
    printf '%s\n' "$BUILD_KEY" > "$OUTPUT/fbink-build/build-config"
fi

printf '%s\n' "$CROSS"

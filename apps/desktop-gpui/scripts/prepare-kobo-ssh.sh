#!/bin/sh
# Optional developer tool; SSH is not enabled by the normal reader installer.
set -eu
SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
REPO_ROOT=$(CDPATH= cd -- "$SCRIPT_DIR/../../.." && pwd)
OUTPUT="$REPO_ROOT/target/desktop-gpui-kobo"
BUILD="$OUTPUT/ssh-build"
VERSION=2026.94
SHA256=e098034a843699200c8c977a991fff73159735bf795d5f72ef672c41a6b1ae81
CROSS="$OUTPUT/toolchain/armv7-eabihf--glibc--stable-2024.05-1/bin/arm-buildroot-linux-gnueabihf"
[ -x "$CROSS-gcc" ] || CROSS=$(sh "$SCRIPT_DIR/prepare-kobo-toolchain.sh")
mkdir -p "$BUILD"
ARCHIVE="$BUILD/dropbear-$VERSION.tar.bz2"
if [ ! -f "$ARCHIVE" ]; then
    curl --fail --location --silent --show-error --retry 3 \
        "https://matt.ucc.asn.au/dropbear/releases/dropbear-$VERSION.tar.bz2" -o "$ARCHIVE.partial"
    mv "$ARCHIVE.partial" "$ARCHIVE"
fi
[ "$(sha256sum "$ARCHIVE" | cut -d ' ' -f 1)" = "$SHA256" ] || { printf 'Dropbear source checksum mismatch\n' >&2; exit 1; }
tar -xjf "$ARCHIVE" -C "$BUILD"
cd "$BUILD/dropbear-$VERSION"
cat >localoptions.h <<'OPTIONS'
#define DROPBEAR_SVR_PASSWORD_AUTH 0
#define DROPBEAR_SVR_PAM_AUTH 0
#define DROPBEAR_SVR_AGENTFWD 0
#define DROPBEAR_X11FWD 0
#define DO_MOTD 0
OPTIONS
CC="$CROSS-gcc" AR="$CROSS-ar" RANLIB="$CROSS-ranlib" \
    CFLAGS='-Os -mcpu=cortex-a9 -mfloat-abi=hard -mfpu=neon' \
    ./configure --host=arm-buildroot-linux-gnueabihf --enable-static --disable-zlib \
        --disable-utmp --disable-utmpx --disable-wtmp --disable-wtmpx \
        --disable-lastlog --disable-syslog >"$BUILD/configure.log" 2>&1
make clean >"$BUILD/clean.log" 2>&1
make -j "${CARGO_BUILD_JOBS:-3}" PROGRAMS='dropbear dropbearkey' >"$BUILD/build.log" 2>&1
"$CROSS-strip" dropbear dropbearkey
mkdir -p "$OUTPUT/ssh"
cp dropbear dropbearkey LICENSE "$OUTPUT/ssh/"
cp "$REPO_ROOT/apps/desktop-gpui/kobo/ssh/"*.sh "$OUTPUT/ssh/"
printf 'Dropbear %s\nSource: https://matt.ucc.asn.au/dropbear/releases/dropbear-%s.tar.bz2\nSHA256: %s\nPassword authentication: disabled at build time\n' \
    "$VERSION" "$VERSION" "$SHA256" >"$OUTPUT/ssh/BUILD-INFO.txt"
cp "$ARCHIVE" "$OUTPUT/ssh/"
printf '%s\n' "$OUTPUT/ssh"

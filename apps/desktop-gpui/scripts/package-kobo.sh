#!/bin/sh
set -eu

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
REPO_ROOT=$(CDPATH= cd -- "$SCRIPT_DIR/../../.." && pwd)
GPUI_FORK=$(CDPATH= cd -- "$REPO_ROOT/../GPUI-Fork" && pwd)
OUTPUT="$REPO_ROOT/target/desktop-gpui-kobo"
FBINK_SOURCE="$REPO_ROOT/target/kobo-toolchain-cache/fbink-source"
FBINK_RELEASE="$FBINK_SOURCE/Release"
BUILD_TARGET=${CARGO_TARGET_DIR:-"$REPO_ROOT/target"}
case "$BUILD_TARGET" in /*) ;; *) BUILD_TARGET="$REPO_ROOT/$BUILD_TARGET" ;; esac

# The default package uses the common ARMv7 hard-float baseline. Libra 2
# tuning remains available for local comparisons, without declaring other models.
KOBO_CPU_PROFILE=${KOBO_CPU_PROFILE:-generic}
case "$KOBO_CPU_PROFILE" in
    libra2)
        KOBO_RUSTFLAGS='-C target-cpu=cortex-a9 -C target-feature=+neon'
        KOBO_CFLAGS='-O3 -mcpu=cortex-a9 -mfpu=neon -mfloat-abi=hard'
        ;;
    generic)
        KOBO_RUSTFLAGS='-C target-cpu=cortex-a8 -C target-feature=+neon'
        KOBO_CFLAGS='-O3 -march=armv7-a -mfpu=neon -mfloat-abi=hard'
        ;;
    *)
        printf 'Unsupported KOBO_CPU_PROFILE: %s\n' "$KOBO_CPU_PROFILE" >&2
        exit 2
        ;;
esac

CROSS=$(KOBO_CFLAGS="$KOBO_CFLAGS" sh "$SCRIPT_DIR/prepare-kobo-toolchain.sh")
SYSROOT=$("$CROSS-gcc" -print-sysroot)
python3 "$REPO_ROOT/shared/pdfium/prepare.py" --target armv7-unknown-linux-gnueabihf >&2

# The firmware's libc is older than current Rust's needs. Use the bundled
# loader and libraries only for this executable; Nickel keeps its own runtime.
KOBO_RUSTFLAGS="$KOBO_RUSTFLAGS -C link-arg=-Wl,--dynamic-linker=/mnt/onboard/.adds/bokheim/runtime/ld-linux-armhf.so.3 -C link-arg=-Wl,-rpath,\$ORIGIN/runtime"

# Keep the cross-build within a desktop machine's memory budget. Large UI
# crates also use smaller code-generation units below. Maintainers with more
# available memory can raise CARGO_BUILD_JOBS explicitly.
CARGO_BUILD_JOBS=${CARGO_BUILD_JOBS:-1}

cd "$REPO_ROOT"
FBINK_LIB_DIR="$FBINK_RELEASE" \
RUSTFLAGS="$KOBO_RUSTFLAGS" \
CFLAGS_armv7_unknown_linux_gnueabihf="$KOBO_CFLAGS" \
CARGO_TARGET_ARMV7_UNKNOWN_LINUX_GNUEABIHF_LINKER="$CROSS-gcc" \
CC_armv7_unknown_linux_gnueabihf="$CROSS-gcc" \
CXX_armv7_unknown_linux_gnueabihf="$CROSS-g++" \
AR_armv7_unknown_linux_gnueabihf="$CROSS-ar" \
CARGO_PROFILE_RELEASE_LTO=thin \
CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1 \
CARGO_BUILD_JOBS="$CARGO_BUILD_JOBS" \
CARGO_PROFILE_RELEASE_DEBUG=0 \
cargo build --release --target-dir "$BUILD_TARGET" --target armv7-unknown-linux-gnueabihf \
    --config 'profile.release.package.gpui-component.codegen-units=16' \
    --config 'profile.release.package.browser-ui.codegen-units=16' \
    --config 'profile.release.package.reader-ui.codegen-units=16' \
    --config 'profile.release.package.application-shell.codegen-units=16' \
    --config 'profile.release.package.desktop-gpui.codegen-units=16' \
    -p desktop-gpui --no-default-features --features kobo --bin desktop-gpui-kobo

mkdir -p "$OUTPUT"
STAGE=$(mktemp -d "$OUTPUT/stage.XXXXXX")
trap 'rm -rf "$STAGE"' EXIT
APP="$STAGE/.adds/bokheim"
mkdir -p "$APP/third-party-source" "$APP/runtime" "$APP/pdfium" "$STAGE/.adds/nm" "$STAGE/Bokheim/Libraries"
mkdir -p "$APP/nickelmenu-install"
cp "$REPO_ROOT/apps/desktop-gpui/kobo/nickelmenu-install/"* "$APP/nickelmenu-install/"
mkdir -p "$APP/command-shims"
cp "$REPO_ROOT/apps/desktop-gpui/kobo/command-shims/bluetoothctl" "$APP/command-shims/bluetoothctl"
cp "$REPO_ROOT/apps/desktop-gpui/kobo/command-timeout.sh" "$APP/command-timeout.sh"
cp "$BUILD_TARGET/armv7-unknown-linux-gnueabihf/release/desktop-gpui-kobo" "$APP/desktop-gpui-kobo"
cp "$BUILD_TARGET/armv7-unknown-linux-gnueabihf/release/taxonomy.sqlite3" "$APP/taxonomy.sqlite3"
# RPATH also covers libraries loaded by PDFium. RUNPATH only covers direct
# dependencies, and exporting LD_LIBRARY_PATH would affect firmware subprocesses.
if ! "$CROSS-readelf" -d "$APP/desktop-gpui-kobo" | grep -F '(RPATH)' | grep -Fq '[$ORIGIN/runtime]'; then
    printf 'Kobo executable is missing its private runtime RPATH\n' >&2
    exit 1
fi
cp "$REPO_ROOT/.pdfium/armv7-unknown-linux-gnueabihf/libpdfium.so" \
    "$REPO_ROOT/.pdfium/armv7-unknown-linux-gnueabihf/LICENSE" \
    "$REPO_ROOT/.pdfium/armv7-unknown-linux-gnueabihf/VERSION" "$APP/pdfium/"
for library in ld-linux-armhf.so.3 libc.so.6 libm.so.6 libgcc_s.so.1 \
    libpthread.so.0 libdl.so.2 librt.so.1 libresolv.so.2 libnss_files.so.2 libnss_dns.so.2; do
    cp -L "$SYSROOT/lib/$library" "$APP/runtime/$library"
done
cp "$REPO_ROOT/shared/pdfium/kobo-runtime/"*.txt "$APP/runtime/"
cp "$FBINK_RELEASE/fbink" "$APP/fbink"
cp "$GPUI_FORK/LICENSE-APACHE" "$APP/LICENSE-APACHE"
cp "$FBINK_SOURCE/LICENSE" "$APP/LICENSE-FBINK-GPLv3"
cp "$GPUI_FORK/assets/fonts/lilex/OFL.txt" "$APP/LICENSE-LILEX-OFL.txt"
cp "$REPO_ROOT/apps/ui/design-tokens/assets/fonts/terminus/TerminusTTF-OFL.txt" "$APP/LICENSE-TERMINUS-OFL.txt"
cp "$REPO_ROOT/vendor/wpa-ctrl/LICENSE" "$APP/LICENSE-WPA-CTRL-BSL-1.0"
python3 - "$REPO_ROOT" "$APP/third-party-source" <<'PY_SOURCES'
import json, shutil, sys
from pathlib import Path
root, output = map(Path, sys.argv[1:])
for name, spec in json.loads((root / 'apps/desktop-gpui/kobo/sources.json').read_text()).items():
    if name != 'toolchain':
        shutil.copyfile(root / 'target/kobo-toolchain-cache' / spec['archive'], output / spec['archive'])
PY_SOURCES
cp "$REPO_ROOT/apps/desktop-gpui/kobo/run.sh" "$APP/run.sh"
cp "$REPO_ROOT/apps/desktop-gpui/kobo/update-recovery.sh" "$APP/update-recovery.sh"
cp "$REPO_ROOT/apps/desktop-gpui/kobo/wifi-power.sh" "$APP/wifi-power.sh"
cp "$REPO_ROOT/apps/desktop-gpui/kobo/run-monitored.sh" "$APP/run-monitored.sh"
cp "$REPO_ROOT/apps/desktop-gpui/kobo/monitor-nickel.sh" "$APP/monitor-nickel.sh"
cp "$REPO_ROOT/apps/desktop-gpui/kobo/probe-kobo-audio.sh" "$APP/probe-kobo-audio.sh"
cp "$REPO_ROOT/apps/desktop-gpui/kobo/nickelmenu" "$STAGE/.adds/nm/desktop-gpui-kobo"
mkdir -p "$APP/nickelmenu-icons"
cp "$REPO_ROOT/apps/ui/design-tokens/assets/icon/bokheim-kobo.png" "$APP/nickelmenu-icons/bokheim.png"
cp "$REPO_ROOT/apps/ui/design-tokens/assets/icon/bokheim-kobo-active.png" "$APP/nickelmenu-icons/bokheim-active.png"
chmod +x "$APP/desktop-gpui-kobo" "$APP/fbink" "$APP/run.sh" "$APP/run-monitored.sh" "$APP/monitor-nickel.sh" "$APP/probe-kobo-audio.sh" "$APP/update-recovery.sh"
chmod +x "$APP/command-shims/bluetoothctl" "$APP/command-timeout.sh"
KOBO_VERSION=$(sed -n 's/^version = "\([^"]*\)"/\1/p' "$REPO_ROOT/apps/desktop-gpui/Cargo.toml" | head -n 1)
printf 'Bokheim shared GPUI UI for Kobo\nVersion: %s\nBuilt: %s\nTarget: armv7-unknown-linux-gnueabihf\nPDFium: 7961 (ARM glibc)\nRuntime: glibc 2.23 (Linux 2.6.32 baseline)\nCPU profile: %s\nRust flags: %s\nC flags: %s\nLTO: thin\nCodegen units: 1\n' \
    "$KOBO_VERSION" "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$KOBO_CPU_PROFILE" "$KOBO_RUSTFLAGS" "$KOBO_CFLAGS" >"$APP/BUILD-INFO.txt"
if [ "$KOBO_CPU_PROFILE" = generic ]; then
    printf 'Supported model IDs: %s\n' "$(cargo run --quiet -p kobo-profile --example devices -- --ids)" >>"$APP/BUILD-INFO.txt"
fi
printf 'FBInk source: %s\n' 886f25f13368859ad8a899b88d04c26e19cda32e >>"$APP/BUILD-INFO.txt"
printf 'UI crate codegen units: 16 (bounded build memory)\n' >>"$APP/BUILD-INFO.txt"
python3 - "${BOKHEIM_BUILD_TAXONOMY_SQLITE:-$REPO_ROOT/shared/subject-projection/data/unified-taxonomy-v2.sqlite3}" >>"$APP/BUILD-INFO.txt" <<'PY'
import hashlib, sys
with open(sys.argv[1], 'rb') as source:
    print('Taxonomy source SHA256: ' + hashlib.file_digest(source, 'sha256').hexdigest())
PY

ARCHIVE="$OUTPUT/desktop-gpui-kobo.tar.gz"
tar -czf "$ARCHIVE" -C "$STAGE" .
printf '%s\n' "$ARCHIVE"

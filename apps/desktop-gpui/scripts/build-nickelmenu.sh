#!/bin/bash
# Build the small NickelMenu payload embedded separately by the USB installer.
set -euo pipefail
SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
REPO_ROOT=$(CDPATH= cd -- "$SCRIPT_DIR/../../.." && pwd)
EXTENSION="$SCRIPT_DIR/../kobo/nickelmenu-direct"
OUTPUT="$SCRIPT_DIR/../kobo/nickelmenu-install"
REVISION=15a95f7e6aa32b053094b062a4157245aecb4cba
VERSION=v0.6.0-bokheim.1

if [ -n "${NICKELTC_ROOT:-}" ]; then
    export PATH="$NICKELTC_ROOT/bin:$PATH"
fi
command -v arm-nickel-linux-gnueabihf-g++ >/dev/null || {
    printf 'NickelTC 1.0 is required; see kobo/nickelmenu-direct/README.md\n' >&2
    exit 1
}
mkdir -p "$REPO_ROOT/target/nickelmenu-direct"
BUILD=$(mktemp -d "$REPO_ROOT/target/nickelmenu-direct/build.XXXXXX")
trap 'rm -rf "$BUILD"' EXIT
git clone --quiet --depth 1 --branch v0.6.0 --recurse-submodules --shallow-submodules \
    https://github.com/pgaskin/NickelMenu.git "$BUILD/source"
[ "$(git -C "$BUILD/source" rev-parse HEAD)" = "$REVISION" ]
git -C "$BUILD/source" apply "$EXTENSION/direct-launch.patch"
cp "$EXTENSION/direct-launch.h" "$BUILD/source/src/direct-launch.h"
cat >>"$BUILD/source/res/doc" <<'DOC'

# Bokheim extension (v0.6.0-bokheim.1):
# experimental:menu_main_15505_direct:<main menu item label>
# Tapping the bottom navigation button runs the uniquely matching main-menu
# item directly. Missing/ambiguous labels and config errors show the usual menu.
# Remove this setting to restore normal menu behavior.
DOC
make -C "$BUILD/source" -j2 all VERSION="$VERSION"
make -C "$BUILD/source" strip
make -C "$BUILD/source" install DESTDIR="$BUILD/stage"
epoch=$(git -C "$BUILD/source" show -s --format=%ct HEAD)
tar --sort=name --mtime="@$epoch" --owner=0 --group=0 --numeric-owner \
    -cf "$BUILD/payload.tar" -C "$BUILD/stage" .
gzip -n -c "$BUILD/payload.tar" >"$BUILD/KoboRoot.tgz"
cp "$BUILD/KoboRoot.tgz" "$OUTPUT/KoboRoot.tgz"
python3 - "$REPO_ROOT" "$VERSION" "$REVISION" <<'PY'
import hashlib
from pathlib import Path
import re
import sys

root, version, revision = Path(sys.argv[1]), sys.argv[2], sys.argv[3]
payload = root / 'apps/desktop-gpui/kobo/nickelmenu-install/KoboRoot.tgz'
digest = hashlib.sha256(payload.read_bytes()).hexdigest()
for relative, pattern, replacement in [
    ('apps/kobo-installer/src/lib.rs', r'(const NICKEL_MENU_SHA256: &str = ")[a-f0-9]{64}(";)', rf'\g<1>{digest}\2'),
    ('apps/desktop-gpui/scripts/install-nickelmenu.sh', r'(SHA256=)[a-f0-9]{64}', rf'\g<1>{digest}'),
]:
    path = root / relative
    content, count = re.subn(pattern, replacement, path.read_text())
    assert count == 1, f'Expected one checksum in {path}'
    path.write_text(content)
(payload.parent / 'VERSION.txt').write_text(
    f'NickelMenu {version}\n'
    f'Based on https://github.com/pgaskin/NickelMenu/commit/{revision}\n'
    'Bokheim direct-launch extension: ../nickelmenu-direct/\n'
    f'KoboRoot.tgz SHA256: {digest}\n'
)
print(f'Updated NickelMenu {version}: {digest}')
PY

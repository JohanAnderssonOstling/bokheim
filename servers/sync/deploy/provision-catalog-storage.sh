#!/usr/bin/env bash
set -euo pipefail

# One-time, opt-in setup. Run again after publication if the publishing tool
# replaces file ACLs. This neither moves nor copies any book bytes.
if [[ ${EUID} -ne 0 || $# -ne 0 ]]; then
    echo "usage: sudo $0" >&2
    exit 2
fi
command -v setfacl >/dev/null || { echo "Install the acl package first." >&2; exit 2; }
id bokheim >/dev/null
catalog_root=/srv/bokheim-updates
catalog_books=${catalog_root}/public/catalog/books
catalog_pins=${catalog_root}/catalog-sync-pins
[[ -d ${catalog_books} ]] || { echo "Public catalog books are missing: ${catalog_books}" >&2; exit 2; }

install -d -o bokheim -g bokheim -m 0700 "${catalog_pins}"
if [[ $(stat -c %d "${catalog_books}") != $(stat -c %d "${catalog_pins}") ]]; then
    echo "Catalog books and private pins must be on the same filesystem." >&2
    exit 2
fi

# Linux protected_hardlinks requires either ownership or read/write access to
# the source inode. Directory access stays read-only; the service never writes
# catalog bytes. Inherited ACLs cover normal publication of future EPUBs.
setfacl -m u:bokheim:--x "${catalog_root}" "${catalog_root}/public" "${catalog_root}/public/catalog"
setfacl -m u:bokheim:rx,d:u:bokheim:rwX "${catalog_books}"
find "${catalog_books}" -maxdepth 1 -regextype posix-extended -type f -regex '.*/[0-9a-f]{64}\.epub' -exec setfacl -m u:bokheim:rw -- {} +

runuser -u bokheim -- python3 - "${catalog_books}" "${catalog_pins}" <<'PY'
import os
import re
import sys
import uuid
from pathlib import Path

books, pins = map(Path, sys.argv[1:])
for book in books.iterdir():
    if re.fullmatch(r"[0-9a-f]{64}\.epub", book.name) and book.is_file() and not book.is_symlink():
        probe = pins / f".provision-{uuid.uuid4()}"
        try:
            os.link(book, probe)
        finally:
            probe.unlink(missing_ok=True)
        break
PY

cat > "${catalog_root}/sync-catalog.env" <<EOF
BOKHEIM_CATALOG_BOOKS_DIR=${catalog_books}
BOKHEIM_CATALOG_PINS_DIR=${catalog_pins}
EOF
chmod 0644 "${catalog_root}/sync-catalog.env"
systemctl daemon-reload
echo "Catalog reuse configured. Restart bokheim-sync after deploying a server with catalog reuse support."

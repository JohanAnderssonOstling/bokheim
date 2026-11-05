#!/bin/sh
# Build the pinned ARM runtime in the local cache; print its compiler prefix.
set -eu
SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
exec python3 "$SCRIPT_DIR/prepare-kobo-runtime.py"

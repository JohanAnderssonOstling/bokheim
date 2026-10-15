#!/usr/bin/env bash
# Install a server binary using the already published immutable answer snapshot.
set -euo pipefail
if [[ $# != 1 || ! -x $1 ]]; then
    echo 'usage: deploy-code.sh /path/to/metadata-server' >&2
    exit 1
fi
answer_root=/home/johan/data/openlibrary
incoming=/var/lib/bokheim-deploy/metadata-incoming
exec 9>"${answer_root}/v3/metadata-release.lock"
flock -n 9 || { echo 'Another metadata release is running' >&2; exit 1; }
test -r "${answer_root}/answers-current.sqlite"
mkdir -- "${incoming}"
trap 'rm -f -- "${incoming}/metadata-server"; rmdir -- "${incoming}" 2>/dev/null || true' EXIT
install -m 0755 -- "$1" "${incoming}/metadata-server"
sudo -n /usr/local/sbin/bokheim-deploy-metadata

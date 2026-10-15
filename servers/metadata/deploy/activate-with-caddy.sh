#!/usr/bin/env bash
# Repair the installed metadata route, then activate the already built release.
set -euo pipefail

if [[ ${EUID} -ne 0 || $# -ne 0 ]]; then
    echo "usage: sudo $0" >&2
    exit 2
fi

installed=/etc/caddy/Caddyfile
caddy_env=/etc/bokheim/caddy.env
incoming=/var/lib/bokheim-deploy/metadata-incoming/metadata-server
helper=/usr/local/sbin/bokheim-deploy-metadata
answers=/home/johan/data/openlibrary/answers-current.sqlite

exec 9>/home/johan/data/openlibrary/v3/metadata-release.lock
flock -n 9 || { echo 'Another metadata release is running' >&2; exit 1; }
for file in "${installed}" "${caddy_env}" "${incoming}" "${helper}"; do
    if [[ ! -f ${file} || -L ${file} ]]; then
        echo "Missing or unsafe deployment file: ${file}" >&2
        exit 1
    fi
done
test -x "${incoming}"
test -x "${helper}"
test -r "${answers}"

temporary=$(mktemp -d /var/tmp/bokheim-metadata-route.XXXXXX)
candidate=${temporary}/Caddyfile
backup="${installed}.backup.$(date -u +%Y%m%dT%H%M%SZ).metadata-v3"
activation_started=false
deployment_succeeded=false
cleanup() {
    result=$?
    trap - EXIT
    if [[ ${activation_started} == true && ${deployment_succeeded} != true ]]; then
        echo "Activation failed; restoring Caddy configuration from ${backup}." >&2
        install -o root -g root -m 0644 "${backup}" "${installed}"
        systemctl reload caddy.service || true
    fi
    rm -rf -- "${temporary}"
    exit "${result}"
}
trap cleanup EXIT

# Preserve every other route and all environment-specific settings.
python3 - "${installed}" "${candidate}" <<'PY'
from pathlib import Path
import re
import sys
source = Path(sys.argv[1]).read_text()
pattern = re.compile(r'(?m)^([ \t]*@metadata_api[ \t]+path[ \t]+)([^\r\n]+)$')
matches = list(pattern.finditer(source))
if len(matches) != 1:
    raise SystemExit('Expected exactly one @metadata_api path matcher; no changes made.')
match = matches[0]
if '/v3/books' not in match.group(2).split():
    source = source[:match.end()] + ' /v3/books' + source[match.end():]
Path(sys.argv[2]).write_text(source)
PY

caddy validate --envfile "${caddy_env}" --config "${candidate}" --adapter caddyfile
cp -a -- "${installed}" "${backup}"
activation_started=true
install -o root -g root -m 0644 "${candidate}" "${installed}"
systemctl reload caddy.service
systemctl is-active --quiet caddy.service

# This fixed helper installs the staged binary and units, checks local/public
# enrichment and CORS, and restores the old service release if a check fails.
"${helper}"
deployment_succeeded=true
echo "Metadata deployed using the existing answer snapshot."
echo "Caddy backup: ${backup}"

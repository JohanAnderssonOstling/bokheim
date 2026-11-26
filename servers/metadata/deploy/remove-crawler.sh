#!/usr/bin/env bash
# Retire the legacy crawler while retaining its provider database and credentials.
set -euo pipefail

script_directory=$(cd -P -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
if [[ ${EUID} -ne 0 ]]; then
    echo "run as root: sudo bash $0" >&2
    exit 2
fi
installer=${script_directory}/install-metadata-release.sh
if [[ ! -f ${installer} || -L ${installer} ]]; then
    echo "install-metadata-release.sh must be a regular file next to this script" >&2
    exit 1
fi
bash -n "${installer}"

exec 9>/run/lock/bokheim-metadata-deploy.lock
flock -n 9 || { echo "another Bokheim metadata deployment is running" >&2; exit 1; }

unit=bokheim-metadata-crawl.service
if systemctl cat "${unit}" >/dev/null 2>&1; then
    systemctl disable --now "${unit}"
fi
rm -f -- "/etc/systemd/system/multi-user.target.wants/${unit}" \
    "/etc/systemd/system/${unit}" \
    "/usr/local/share/bokheim-deploy/${unit}"
install -o root -g root -m 0755 "${installer}" /usr/local/sbin/bokheim-deploy-metadata
systemctl daemon-reload
systemctl reset-failed "${unit}" 2>/dev/null || true
echo "Metadata crawler removed; collected provider data retained."

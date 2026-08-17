#!/usr/bin/env bash
set -euo pipefail

deploy_user=${1:-${SUDO_USER:-}}
script_directory=$(cd -P -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
repository_root=$(cd -P -- "${script_directory}/../../.." && pwd)
catalog_service=${repository_root}/servers/opds-cache/deploy/bokheim-catalogs.service
if [[ ${EUID} -ne 0 ]]; then
    echo "run once as root: sudo $0 deployment-user" >&2
    exit 2
fi
if [[ -z ${deploy_user} ]] || ! id "${deploy_user}" >/dev/null 2>&1; then
    echo "a valid deployment user is required" >&2
    exit 2
fi
for required in install-metadata-catalog-release.sh bokheim-metadata.service; do
    if [[ ! -f ${script_directory}/${required} || -L ${script_directory}/${required} ]]; then
        echo "${required} must be a regular file next to this provisioner" >&2
        exit 1
    fi
done
if [[ ! -f ${catalog_service} || -L ${catalog_service} ]]; then
    echo "bokheim-catalogs.service must be a regular file at ${catalog_service}" >&2
    exit 1
fi

deploy_group=bokheim-server-deploy
getent group "${deploy_group}" >/dev/null || groupadd --system "${deploy_group}"
usermod -aG "${deploy_group}" "${deploy_user}"
install -d -o root -g "${deploy_group}" -m 3771 /var/lib/bokheim-deploy
install -o root -g root -m 0755 \
    "${script_directory}/install-metadata-catalog-release.sh" \
    /usr/local/sbin/bokheim-deploy-metadata-catalog
install -d -o root -g root -m 0755 /usr/local/share/bokheim-deploy
install -o root -g root -m 0644 \
    "${script_directory}/bokheim-metadata.service" \
    /usr/local/share/bokheim-deploy/bokheim-metadata.service
install -o root -g root -m 0644 \
    "${catalog_service}" \
    /usr/local/share/bokheim-deploy/bokheim-catalogs.service

sudoers_file=/etc/sudoers.d/bokheim-metadata-catalog-deploy
printf '%s ALL=(root) NOPASSWD: /usr/local/sbin/bokheim-deploy-metadata-catalog\n' "${deploy_user}" >"${sudoers_file}"
chown root:root "${sudoers_file}"
chmod 0440 "${sudoers_file}"
visudo -cf "${sudoers_file}" >/dev/null

echo "Metadata/catalogue deployment access granted to ${deploy_user}."
echo "Reconnect SSH if ${deploy_user} was not already in ${deploy_group}."

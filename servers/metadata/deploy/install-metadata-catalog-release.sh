#!/usr/bin/env bash
set -euo pipefail

if [[ ${EUID} -ne 0 || $# -ne 0 ]]; then
    echo "usage: sudo /usr/local/sbin/bokheim-deploy-metadata-catalog" >&2
    exit 2
fi

exec 9>/run/lock/bokheim-metadata-catalog-deploy.lock
flock -n 9 || { echo "another Bokheim metadata/catalogue deployment is running" >&2; exit 1; }

source_root=/var/lib/bokheim-deploy/metadata-catalog-incoming
service_templates=/usr/local/share/bokheim-deploy
for required in metadata-server catalog-server opds.json; do
    if [[ ! -f ${source_root}/${required} || -L ${source_root}/${required} ]]; then
        echo "missing or unsafe staged artifact: ${required}" >&2
        exit 1
    fi
done
if [[ ! -x ${source_root}/metadata-server || ! -x ${source_root}/catalog-server ]]; then
    echo "staged server binaries must be executable" >&2
    exit 1
fi
if [[ ! -r /home/johan/data/openlibrary/current.sqlite ]]; then
    echo "missing verified metadata snapshot" >&2
    exit 1
fi
for required in bokheim-metadata.service bokheim-catalogs.service; do
    if [[ ! -f ${service_templates}/${required} || -L ${service_templates}/${required} || $(stat -c %u "${service_templates}/${required}") -ne 0 ]]; then
        echo "missing or unsafe root-owned service template: ${required}" >&2
        exit 1
    fi
done

rollback_root=$(mktemp -d /var/tmp/bokheim-metadata-catalog-rollback.XXXXXX)
metadata_binary_existed=false
metadata_unit_existed=false
activation_started=false
deployment_succeeded=false
[[ -f /usr/local/bin/metadata-server ]] && metadata_binary_existed=true
[[ -f /etc/systemd/system/bokheim-metadata.service ]] && metadata_unit_existed=true
for installed in /usr/local/bin/catalog-server /usr/local/bin/metadata-server /usr/local/share/catalog-providers/opds.json /etc/systemd/system/bokheim-catalogs.service /etc/systemd/system/bokheim-metadata.service; do
    if [[ -f ${installed} ]]; then
        cp -a -- "${installed}" "${rollback_root}/"
    fi
done
cleanup() {
    status=$?
    trap - EXIT
    if [[ ${activation_started} == true && ${deployment_succeeded} != true ]]; then
        echo "activation failed; restoring the previous catalogue release" >&2
        [[ -f ${rollback_root}/catalog-server ]] && install -o root -g root -m 0755 "${rollback_root}/catalog-server" /usr/local/bin/catalog-server
        [[ -f ${rollback_root}/opds.json ]] && install -o root -g root -m 0644 "${rollback_root}/opds.json" /usr/local/share/catalog-providers/opds.json
        [[ -f ${rollback_root}/bokheim-catalogs.service ]] && install -o root -g root -m 0644 "${rollback_root}/bokheim-catalogs.service" /etc/systemd/system/bokheim-catalogs.service
        if [[ ${metadata_binary_existed} == true ]]; then
            install -o root -g root -m 0755 "${rollback_root}/metadata-server" /usr/local/bin/metadata-server
        else
            rm -f -- /usr/local/bin/metadata-server
        fi
        if [[ ${metadata_unit_existed} == true ]]; then
            install -o root -g root -m 0644 "${rollback_root}/bokheim-metadata.service" /etc/systemd/system/bokheim-metadata.service
        else
            systemctl disable --now bokheim-metadata.service 2>/dev/null || true
            rm -f -- /etc/systemd/system/bokheim-metadata.service
        fi
        systemctl daemon-reload
        systemctl restart bokheim-catalogs.service || true
    fi
    rm -rf -- "${rollback_root}"
    exit "${status}"
}
trap cleanup EXIT

activation_started=true
systemctl stop bokheim-catalogs.service bokheim-metadata.service 2>/dev/null || true
# Do not use a process-name-wide cleanup here. Linux truncates `comm` names,
# which can make a long-running metadata importer look exactly like the server
# and would interrupt an unrelated resumable snapshot build.
install -o root -g root -m 0755 "${source_root}/metadata-server" /usr/local/bin/metadata-server
install -o root -g root -m 0755 "${source_root}/catalog-server" /usr/local/bin/catalog-server
install -d -o root -g root -m 0755 /usr/local/share/catalog-providers
install -o root -g root -m 0644 "${source_root}/opds.json" /usr/local/share/catalog-providers/opds.json
install -o root -g root -m 0644 "${service_templates}/bokheim-metadata.service" /etc/systemd/system/bokheim-metadata.service
install -o root -g root -m 0644 "${service_templates}/bokheim-catalogs.service" /etc/systemd/system/bokheim-catalogs.service
systemctl daemon-reload
systemctl enable bokheim-metadata.service bokheim-catalogs.service
systemctl restart bokheim-metadata.service
systemctl restart bokheim-catalogs.service

metadata_ready=false
catalog_ready=false
for _ in $(seq 1 30); do
    if curl --noproxy '*' --fail --silent --max-time 3 http://127.0.0.1:8091/health | grep -Fqx ok; then metadata_ready=true; fi
    if curl --noproxy '*' --fail --silent --max-time 3 http://127.0.0.1:8090/health | grep -Fqx ok; then catalog_ready=true; fi
    if [[ ${metadata_ready} == true && ${catalog_ready} == true ]]; then break; fi
    sleep 1
done
if [[ ${metadata_ready} != true || ${catalog_ready} != true ]]; then
    echo "metadata or catalogue service did not become healthy" >&2
    exit 1
fi

curl --noproxy '*' --fail --silent --show-error --max-time 15 \
    -H 'Content-Type: application/json' \
    --data '{"isbns":["9780821338278"]}' \
    http://127.0.0.1:8091/v2/enrichment >/dev/null

deployment_succeeded=true
rm -rf -- "${source_root}"
find /var/lib/bokheim-deploy -mindepth 1 -maxdepth 1 -type d -name 'metadata-catalog-previous.*' -exec rm -rf -- {} +
echo "Bokheim metadata and catalogue services installed, healthy, and enrichment-verified."

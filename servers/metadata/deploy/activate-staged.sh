#!/usr/bin/env bash
set -euo pipefail

source_root=${1:-}
if [[ ${EUID} -ne 0 || -z ${source_root} || ${source_root} != /* ]]; then
    echo "usage: sudo $0 /absolute/staged-release-directory" >&2
    exit 2
fi
for required in pdfium/libpdfium.so pdfium/LICENSE metadata-server catalog-server opds.json bokheim-metadata.service bokheim-catalogs.service; do
    if [[ ! -f ${source_root}/${required} || -L ${source_root}/${required} ]]; then
        echo "missing or unsafe staged artifact: ${source_root}/${required}" >&2
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
"${source_root}/catalog-server" --check-catalog "${source_root}/opds.json"

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
if [[ -d /usr/local/bin/pdfium ]]; then cp -a /usr/local/bin/pdfium "${rollback_root}/pdfium"; fi
cleanup() {
    status=$?
    trap - EXIT
    if [[ ${activation_started} == true && ${deployment_succeeded} != true ]]; then
        echo "activation failed; restoring the previous catalogue release" >&2
        if [[ -d ${rollback_root}/pdfium ]]; then install -o root -g root -m 0644 "${rollback_root}/pdfium/libpdfium.so" "${rollback_root}/pdfium/LICENSE" /usr/local/bin/pdfium/; fi
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
# Stop the temporary unprivileged verifier, if it is still bound to port 8091.
pkill -u johan -x metadata-server 2>/dev/null || true
install -o root -g root -m 0755 "${source_root}/metadata-server" /usr/local/bin/metadata-server
install -o root -g root -m 0755 "${source_root}/catalog-server" /usr/local/bin/catalog-server
install -d -o root -g root -m 0755 /usr/local/bin/pdfium
install -o root -g root -m 0644 "${source_root}/pdfium/libpdfium.so" /usr/local/bin/pdfium/libpdfium.so
install -o root -g root -m 0644 "${source_root}/pdfium/LICENSE" /usr/local/bin/pdfium/LICENSE
install -d -o root -g root -m 0755 /usr/local/share/catalog-providers
install -o root -g root -m 0644 "${source_root}/opds.json" /usr/local/share/catalog-providers/opds.json
install -o root -g root -m 0644 "${source_root}/bokheim-metadata.service" /etc/systemd/system/bokheim-metadata.service
install -o root -g root -m 0644 "${source_root}/bokheim-catalogs.service" /etc/systemd/system/bokheim-catalogs.service
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
echo "Bokheim metadata and catalogue services installed, healthy, and enrichment-verified."

#!/usr/bin/env bash
set -euo pipefail

source_root=${1:-}
if [[ ${EUID} -ne 0 ]]; then
    echo "run this installer as root: sudo $0 staged-artifact-directory" >&2
    exit 2
fi
if [[ -z ${source_root} || ${source_root} != /* ]]; then
    echo "staged artifact directory must be an absolute path" >&2
    exit 2
fi

for required in pdfium/libpdfium.so pdfium/LICENSE catalog-server opds.json bokheim-catalogs.service; do
    if [[ ! -f ${source_root}/${required} ]]; then
        echo "missing staged deployment file: ${source_root}/${required}" >&2
        exit 1
    fi
done
if [[ ! -x ${source_root}/catalog-server ]]; then
    echo "staged catalog server must be executable" >&2
    exit 1
fi

"${source_root}/catalog-server" --check-catalog "${source_root}/opds.json"

rollback_root=$(mktemp -d /tmp/bokheim-catalog-rollback.XXXXXX)
cleanup() {
    rm -rf -- "${rollback_root}"
}
trap cleanup EXIT

cp --preserve=mode,ownership,timestamps /usr/local/bin/catalog-server "${rollback_root}/catalog-server"
cp --preserve=mode,ownership,timestamps /usr/local/share/catalog-providers/opds.json "${rollback_root}/opds.json"
cp --preserve=mode,ownership,timestamps /etc/systemd/system/bokheim-catalogs.service "${rollback_root}/bokheim-catalogs.service"

if [[ -d /usr/local/bin/pdfium ]]; then cp -a /usr/local/bin/pdfium "${rollback_root}/pdfium"; fi

systemctl stop bokheim-catalogs.service
install -o root -g root -m 0755 "${source_root}/catalog-server" /usr/local/bin/catalog-server
install -d -o root -g root -m 0755 /usr/local/bin/pdfium
install -o root -g root -m 0644 "${source_root}/pdfium/libpdfium.so" /usr/local/bin/pdfium/libpdfium.so
install -o root -g root -m 0644 "${source_root}/pdfium/LICENSE" /usr/local/bin/pdfium/LICENSE
install -d -o root -g root -m 0755 /usr/local/share/catalog-providers
install -o root -g root -m 0644 "${source_root}/opds.json" /usr/local/share/catalog-providers/opds.json
install -o root -g root -m 0644 "${source_root}/bokheim-catalogs.service" /etc/systemd/system/bokheim-catalogs.service
systemctl daemon-reload
systemctl restart bokheim-catalogs.service

healthy=false
for _ in $(seq 1 30); do
    if curl --fail --silent --show-error --max-time 5 http://127.0.0.1:8090/health | grep -Fqx ok; then
        healthy=true
        break
    fi
    sleep 1
done

if [[ ${healthy} != true ]]; then
    echo "catalogue deployment did not become healthy; restoring the previous release" >&2
    if [[ -d ${rollback_root}/pdfium ]]; then install -o root -g root -m 0644 "${rollback_root}/pdfium/libpdfium.so" "${rollback_root}/pdfium/LICENSE" /usr/local/bin/pdfium/; fi
    install -o root -g root -m 0755 "${rollback_root}/catalog-server" /usr/local/bin/catalog-server
    install -o root -g root -m 0644 "${rollback_root}/opds.json" /usr/local/share/catalog-providers/opds.json
    install -o root -g root -m 0644 "${rollback_root}/bokheim-catalogs.service" /etc/systemd/system/bokheim-catalogs.service
    systemctl daemon-reload
    systemctl restart bokheim-catalogs.service
    systemctl --no-pager --full status bokheim-catalogs.service >&2 || true
    exit 1
fi

echo "Bokheim catalogue server installed and healthy."
systemctl --no-pager --full status bokheim-catalogs.service

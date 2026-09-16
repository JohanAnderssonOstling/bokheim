#!/usr/bin/env bash
set -euo pipefail

if [[ ${EUID} -ne 0 || $# -ne 0 ]]; then
    echo "usage: sudo /usr/local/sbin/bokheim-deploy-servers" >&2
    exit 2
fi

exec 9>/run/lock/bokheim-server-deploy.lock
flock -n 9 || { echo "another Bokheim server deployment is running" >&2; exit 1; }

incoming=/var/lib/bokheim-deploy/incoming
for file in pdfium/libpdfium.so pdfium/LICENSE sync-server catalog-server metadata-server opds.json bokheim-sync.service bokheim-catalogs.service bokheim-metadata.service; do
    if [[ ! -f ${incoming}/${file} || -L ${incoming}/${file} ]]; then
        echo "missing or unsafe staged artifact: ${file}" >&2
        exit 1
    fi
done
if [[ ! -x ${incoming}/sync-server || ! -x ${incoming}/catalog-server || ! -x ${incoming}/metadata-server ]]; then
    echo "staged server binaries must be executable" >&2
    exit 1
fi
if [[ ! -r /home/johan/data/openlibrary/current.sqlite ]]; then
    echo "missing Open Library snapshot: /home/johan/data/openlibrary/current.sqlite" >&2
    exit 2
fi

for credential in database-url account-token-key admin-password resend-api-key test-user-password; do
    if [[ ! -s /etc/bokheim/${credential} ]]; then
        echo "missing credential: /etc/bokheim/${credential}" >&2
        exit 2
    fi
done
database_url=$(</etc/bokheim/database-url)
if [[ ! ${database_url} =~ ^postgresql://bokheim_app:[[:xdigit:]]{64}@127\.0\.0\.1/bokheim$ ]]; then
    echo "database credential does not identify the expected local Bokheim database" >&2
    exit 2
fi

candidate=$(mktemp -d /var/lib/bokheim-deploy/candidate.XXXXXX)
backup=$(mktemp -d /var/lib/bokheim-deploy/backup.XXXXXX)
chmod 0755 "${candidate}"
activation_started=false
deployment_succeeded=false
metadata_binary_preexisting=false
metadata_unit_preexisting=false
[[ -f /usr/local/bin/metadata-server ]] && metadata_binary_preexisting=true
[[ -f /etc/systemd/system/bokheim-metadata.service ]] && metadata_unit_preexisting=true
cleanup() {
    status=$?
    trap - EXIT
    if [[ ${activation_started} == true && ${deployment_succeeded} != true ]]; then
        echo "restoring the previous server executables" >&2
        if [[ -d ${backup}/pdfium ]]; then install -o root -g root -m 0644 "${backup}/pdfium/libpdfium.so" "${backup}/pdfium/LICENSE" /usr/local/bin/pdfium/; fi
        [[ -f ${backup}/sync-server ]] && install -o root -g root -m 0755 "${backup}/sync-server" /usr/local/bin/sync-server
        [[ -f ${backup}/catalog-server ]] && install -o root -g root -m 0755 "${backup}/catalog-server" /usr/local/bin/catalog-server
        [[ -f ${backup}/metadata-server ]] && install -o root -g root -m 0755 "${backup}/metadata-server" /usr/local/bin/metadata-server
        [[ -f ${backup}/opds.json ]] && install -o root -g root -m 0644 "${backup}/opds.json" /usr/local/share/catalog-providers/opds.json
        [[ -f ${backup}/bokheim-sync.service ]] && install -o root -g root -m 0644 "${backup}/bokheim-sync.service" /etc/systemd/system/bokheim-sync.service
        [[ -f ${backup}/bokheim-catalogs.service ]] && install -o root -g root -m 0644 "${backup}/bokheim-catalogs.service" /etc/systemd/system/bokheim-catalogs.service
        [[ -f ${backup}/bokheim-metadata.service ]] && install -o root -g root -m 0644 "${backup}/bokheim-metadata.service" /etc/systemd/system/bokheim-metadata.service
        if [[ ${metadata_unit_preexisting} == false ]]; then
            systemctl disable --now bokheim-metadata.service || true
            rm -f -- /etc/systemd/system/bokheim-metadata.service
        fi
        [[ ${metadata_binary_preexisting} == false ]] && rm -f -- /usr/local/bin/metadata-server
        systemctl daemon-reload || true
        if [[ -n ${database_url:-} && -x /usr/local/bin/sync-server ]]; then
            runuser -u bokheim -- env DATABASE_URL="${database_url}" /usr/local/bin/sync-server migrate || true
        fi
        systemctl restart bokheim-metadata.service bokheim-sync.service bokheim-catalogs.service || true
    fi
    rm -rf -- "${candidate}" "${backup}"
    exit "${status}"
}
trap cleanup EXIT
install -o root -g root -m 0755 "${incoming}/sync-server" "${candidate}/sync-server"
install -o root -g root -m 0755 "${incoming}/catalog-server" "${candidate}/catalog-server"
install -d -o root -g root -m 0755 "${candidate}/pdfium"
install -o root -g root -m 0644 "${incoming}/pdfium/libpdfium.so" "${candidate}/pdfium/libpdfium.so"
install -o root -g root -m 0644 "${incoming}/pdfium/LICENSE" "${candidate}/pdfium/LICENSE"
install -o root -g root -m 0755 "${incoming}/metadata-server" "${candidate}/metadata-server"
install -o root -g root -m 0644 "${incoming}/opds.json" "${candidate}/opds.json"
install -o root -g root -m 0644 "${incoming}/bokheim-sync.service" "${candidate}/bokheim-sync.service"
install -o root -g root -m 0644 "${incoming}/bokheim-catalogs.service" "${candidate}/bokheim-catalogs.service"
install -o root -g root -m 0644 "${incoming}/bokheim-metadata.service" "${candidate}/bokheim-metadata.service"
runuser -u bokheim -- "${candidate}/catalog-server" --check-catalog "${candidate}/opds.json"

for installed in /usr/local/bin/sync-server /usr/local/bin/catalog-server /usr/local/bin/metadata-server /usr/local/share/catalog-providers/opds.json /etc/systemd/system/bokheim-sync.service /etc/systemd/system/bokheim-catalogs.service /etc/systemd/system/bokheim-metadata.service; do
    if [[ -f ${installed} ]]; then cp -a -- "${installed}" "${backup}/"; fi
done

if [[ -d /usr/local/bin/pdfium ]]; then cp -a /usr/local/bin/pdfium "${backup}/pdfium"; fi

activation_started=true
systemctl stop bokheim-sync.service bokheim-catalogs.service bokheim-metadata.service 2>/dev/null || true
install -o root -g root -m 0755 "${candidate}/sync-server" /usr/local/bin/sync-server
install -o root -g root -m 0755 "${candidate}/catalog-server" /usr/local/bin/catalog-server
install -d -o root -g root -m 0755 /usr/local/bin/pdfium
install -o root -g root -m 0644 "${candidate}/pdfium/libpdfium.so" /usr/local/bin/pdfium/libpdfium.so
install -o root -g root -m 0644 "${candidate}/pdfium/LICENSE" /usr/local/bin/pdfium/LICENSE
install -o root -g root -m 0755 "${candidate}/metadata-server" /usr/local/bin/metadata-server
install -d -o root -g root -m 0755 /usr/local/share/catalog-providers
install -o root -g root -m 0644 "${candidate}/opds.json" /usr/local/share/catalog-providers/opds.json
install -o root -g root -m 0644 "${candidate}/bokheim-sync.service" /etc/systemd/system/bokheim-sync.service
install -o root -g root -m 0644 "${candidate}/bokheim-catalogs.service" /etc/systemd/system/bokheim-catalogs.service
install -o root -g root -m 0644 "${candidate}/bokheim-metadata.service" /etc/systemd/system/bokheim-metadata.service
systemctl daemon-reload
systemctl enable bokheim-metadata.service bokheim-catalogs.service

# Deployments intentionally reset metadata; content-addressed assets remain.
runuser -u postgres -- dropdb --if-exists --force bokheim
runuser -u postgres -- createdb --owner=bokheim_app bokheim
runuser -u bokheim -- env DATABASE_URL="${database_url}" /usr/local/bin/sync-server migrate

systemctl restart bokheim-metadata.service
systemctl restart bokheim-sync.service
systemctl restart bokheim-catalogs.service

sync_ready=false
catalog_ready=false
metadata_ready=false
for _ in $(seq 1 30); do
    if curl --fail --silent --unix-socket /run/bokheim/sync.sock http://localhost/api/ready | grep -Fqx ok; then sync_ready=true; fi
    if curl --fail --silent http://127.0.0.1:8090/health | grep -Fqx ok; then catalog_ready=true; fi
    if curl --fail --silent http://127.0.0.1:8091/health | grep -Fqx ok; then metadata_ready=true; fi
    if [[ ${sync_ready} == true && ${catalog_ready} == true && ${metadata_ready} == true ]]; then break; fi
    sleep 1
done
if [[ ${sync_ready} != true || ${catalog_ready} != true || ${metadata_ready} != true ]]; then
    echo "deployment did not become healthy" >&2
    exit 1
fi

deployment_succeeded=true
unset database_url
rm -rf -- "${incoming}"
find /var/lib/bokheim-deploy -mindepth 1 -maxdepth 1 -type d -name 'previous.*' -exec rm -rf -- {} +
echo "Bokheim sync, catalogue, and metadata servers deployed and healthy."

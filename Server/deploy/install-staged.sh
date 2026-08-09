#!/usr/bin/env bash
set -euo pipefail

source_root=${1:-/home/johan/bokheim-deploy/incoming}

if [[ ${EUID} -ne 0 ]]; then
    echo "run this installer as root: sudo $0 [staged-artifact-directory]" >&2
    exit 2
fi

if [[ ! ${source_root} = /* ]]; then
    echo "staged artifact directory must be an absolute path" >&2
    exit 2
fi

required_files=(
    sync-server
    bokheim-catalog-server
    opds.json
    bokheim-sync.service
    bokheim-catalogs.service
    bokheim-gc.service
    bokheim-gc.timer
    bokheim-tmpfiles.conf
)

for file in "${required_files[@]}"; do
    if [[ ! -f ${source_root}/${file} ]]; then
        echo "missing staged deployment file: ${source_root}/${file}" >&2
        exit 1
    fi
done

if [[ ! -x ${source_root}/sync-server || ! -x ${source_root}/bokheim-catalog-server ]]; then
    echo "staged server binaries must be executable" >&2
    exit 1
fi

install -o root -g root -m 0755 "${source_root}/sync-server" \
    /usr/local/bin/sync-server
install -o root -g root -m 0755 "${source_root}/bokheim-catalog-server" \
    /usr/local/bin/bokheim-catalog-server

install -d -o root -g root -m 0755 /usr/local/share/bokheim-catalogs
install -o root -g root -m 0644 "${source_root}/opds.json" \
    /usr/local/share/bokheim-catalogs/opds.json

install -o root -g root -m 0644 "${source_root}/bokheim-sync.service" \
    /etc/systemd/system/bokheim-sync.service
install -o root -g root -m 0644 "${source_root}/bokheim-catalogs.service" \
    /etc/systemd/system/bokheim-catalogs.service
install -o root -g root -m 0644 "${source_root}/bokheim-gc.service" \
    /etc/systemd/system/bokheim-gc.service
install -o root -g root -m 0644 "${source_root}/bokheim-gc.timer" \
    /etc/systemd/system/bokheim-gc.timer
install -d -o root -g root -m 0755 /etc/tmpfiles.d
install -o root -g root -m 0644 "${source_root}/bokheim-tmpfiles.conf" \
    /etc/tmpfiles.d/bokheim.conf
systemd-tmpfiles --create /etc/tmpfiles.d/bokheim.conf

systemctl daemon-reload
systemctl restart bokheim-sync.service
systemctl restart bokheim-catalogs.service

sync_ready=false
catalog_ready=false
for _ in $(seq 1 30); do
    if curl --fail --silent --show-error \
        --unix-socket /run/bokheim/sync.sock \
        http://localhost/api/ready | grep -Fqx ok; then
        sync_ready=true
    fi
    if curl --fail --silent --show-error \
        http://127.0.0.1:8090/health | grep -Fqx ok; then
        catalog_ready=true
    fi
    if [[ ${sync_ready} == true && ${catalog_ready} == true ]]; then
        break
    fi
    sleep 1
done

if [[ ${sync_ready} != true || ${catalog_ready} != true ]]; then
    echo "deployment did not become healthy" >&2
    systemctl --no-pager --full status bokheim-sync.service bokheim-catalogs.service >&2 || true
    exit 1
fi

echo "Bokheim server deployment installed and healthy."
systemctl --no-pager --full status bokheim-sync.service bokheim-catalogs.service

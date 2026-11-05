#!/usr/bin/env bash
set -euo pipefail
umask 077

if [[ ${EUID} -ne 0 || $# -ne 0 ]]; then
    echo "usage: sudo /usr/local/sbin/bokheim-deploy-sync" >&2
    exit 2
fi
exec 9>/run/lock/bokheim-sync-deploy.lock
flock -n 9 || { echo "another Bokheim sync deployment is running" >&2; exit 1; }
incoming=/var/lib/bokheim-deploy/sync-incoming
for file in sync-server library-cloud-upgrade bokheim-sync.service protocol.json; do
    [[ -f ${incoming}/${file} && ! -L ${incoming}/${file} ]] || { echo "missing or unsafe staged artifact: ${file}" >&2; exit 1; }
done
for file in sync-server library-cloud-upgrade; do
    [[ -x ${incoming}/${file} ]] || { echo "staged executable is not executable: ${file}" >&2; exit 1; }
done
for credential in database-url resend-api-key test-user-password account-token-key; do
    [[ -s /etc/bokheim/${credential} ]] || { echo "missing credential: ${credential}" >&2; exit 2; }
done
database_url=$(</etc/bokheim/database-url)
[[ ${database_url} =~ ^postgresql://bokheim_app:[[:xdigit:]]{64}@127\.0\.0\.1/bokheim$ ]] || { echo "unexpected database credential" >&2; exit 2; }

candidate=$(mktemp -d /var/tmp/bokheim-sync-candidate.XXXXXX)
chmod 0755 "${candidate}"
for file in sync-server library-cloud-upgrade; do
    install -o root -g root -m 0755 "${incoming}/${file}" "${candidate}/${file}"
done
install -o root -g root -m 0644 "${incoming}/bokheim-sync.service" "${candidate}/bokheim-sync.service"
install -o root -g root -m 0644 "${incoming}/protocol.json" "${candidate}/protocol.json"
grep -Fqx 'RuntimeDirectoryMode=0750' "${candidate}/bokheim-sync.service"
grep -Fqx 'Environment=SYNC_SERVER_SOCKET=/run/bokheim/sync.sock' "${candidate}/bokheim-sync.service"
maintenance=/etc/systemd/system/bokheim-sync.service.d/99-deploy-maintenance.conf
[[ ! -e ${maintenance} ]] || { echo "another maintenance override is present" >&2; exit 1; }
backup=
service_stopped=false
migration_started=false
traffic_open=false
maintenance_created=false
cleanup() {
    status=$?
    trap - EXIT
    if [[ ${service_stopped} == true && ${traffic_open} != true ]]; then
        echo "activation failed before publication; restoring the previous database and release" >&2
        systemctl stop bokheim-sync.service || true
        if [[ ${migration_started} == true ]]; then
            if [[ $(<"${backup}/archive-existed") == f ]]; then
                runuser -u postgres -- psql -X -v ON_ERROR_STOP=1 -d bokheim -c 'DROP SCHEMA IF EXISTS bokheim_upgrade_archive CASCADE' >/dev/null
            fi
            runuser -u postgres -- pg_restore --clean --if-exists --exit-on-error --dbname=bokheim <"${backup}/database.dump"
        fi
        install -o root -g root -m 0755 "${backup}/sync-server" /usr/local/bin/sync-server
        install -o root -g root -m 0644 "${backup}/bokheim-sync.service" /etc/systemd/system/bokheim-sync.service
        if [[ -f ${backup}/library-cloud-upgrade ]]; then
            install -o root -g root -m 0755 "${backup}/library-cloud-upgrade" /usr/local/bin/library-cloud-upgrade
        else
            rm -f /usr/local/bin/library-cloud-upgrade
        fi
        if [[ ${maintenance_created} == true ]]; then rm -f "${maintenance}"; maintenance_created=false; fi
        systemctl daemon-reload
        systemctl reset-failed bokheim-sync.service || true
        systemctl start bokheim-sync.service
    elif [[ ${status} != 0 && ${traffic_open} == true ]]; then
        echo "release is accepting traffic; database rollback was not attempted after publication" >&2
    fi
    if [[ ${maintenance_created} == true ]]; then rm -f "${maintenance}"; systemctl daemon-reload; fi
    rm -rf -- "${candidate}"
    unset database_url
    exit "${status}"
}
trap cleanup EXIT

# Reject unreadable client records while the existing release is still serving.
runuser -u bokheim -- env DATABASE_URL="${database_url}" "${candidate}/sync-server" check-upgrade
media_type=$(python3 - "${candidate}/protocol.json" "${candidate}/protocol-probe.bin" <<'PYTHON'
import json,pathlib,sys
p=json.loads(pathlib.Path(sys.argv[1]).read_text())
v=p['version'];mime=p['media_type']
assert type(v) is int and 0<v<=0xffffffff and '/' in mime and not any(c.isspace() or c in ';\r\n' for c in mime)
b=bytearray([8]);n=v
while n>=128:b.append((n&127)|128);n>>=7
b.append(n);pathlib.Path(sys.argv[2]).write_bytes(b)
print(f'{mime}; version={v}')
PYTHON
)
install -d -o root -g root -m 0700 /var/backups/bokheim-sync
backup=$(mktemp -d /var/backups/bokheim-sync/release.XXXXXX)
cp -a /usr/local/bin/sync-server "${backup}/sync-server"
cp -a /etc/systemd/system/bokheim-sync.service "${backup}/bokheim-sync.service"
if [[ -f /usr/local/bin/library-cloud-upgrade ]]; then cp -a /usr/local/bin/library-cloud-upgrade "${backup}/library-cloud-upgrade"; fi
runuser -u postgres -- psql -X -A -t -v ON_ERROR_STOP=1 -d bokheim -c "SELECT EXISTS(SELECT 1 FROM pg_namespace WHERE nspname='bokheim_upgrade_archive')" >"${backup}/archive-existed"
install -d -o root -g root -m 0755 "$(dirname "${maintenance}")"
printf '[Service]\nRuntimeDirectoryMode=0700\n' >"${maintenance}"
chmod 0644 "${maintenance}"
maintenance_created=true
systemctl daemon-reload
service_stopped=true
systemctl stop bokheim-sync.service
runuser -u postgres -- pg_dump --format=custom --dbname=bokheim >"${backup}/database.dump"
pg_restore --list "${backup}/database.dump" >/dev/null
migration_started=true
runuser -u bokheim -- env DATABASE_URL="${database_url}" "${candidate}/sync-server" upgrade
runuser -u bokheim -- env DATABASE_URL="${database_url}" "${candidate}/sync-server" verify-schema
runuser -u bokheim -- env DATABASE_URL="${database_url}" "${candidate}/sync-server" check-upgrade
install -o root -g root -m 0755 "${candidate}/sync-server" /usr/local/bin/sync-server
install -o root -g root -m 0755 "${candidate}/library-cloud-upgrade" /usr/local/bin/library-cloud-upgrade
install -o root -g root -m 0644 "${candidate}/bokheim-sync.service" /etc/systemd/system/bokheim-sync.service
systemctl daemon-reload
systemctl reset-failed bokheim-sync.service || true
systemctl start bokheim-sync.service
ready=false
for _ in $(seq 1 30); do
    if curl --fail --silent --unix-socket /run/bokheim/sync.sock http://localhost/api/ready | grep -Fqx ok; then ready=true; break; fi
    sleep 1
done
[[ ${ready} == true ]] || { echo "candidate did not become ready" >&2; exit 1; }
# Caddy cannot enter the private runtime directory until these checks pass.
[[ $(stat -c %a /run/bokheim) == 700 ]] || { echo "candidate runtime directory is not private" >&2; exit 1; }
protocol_status=$(curl --silent --output /dev/null --write-out '%{http_code}' --max-time 10 --unix-socket /run/bokheim/sync.sock -H "Content-Type: ${media_type}" -H "Accept: ${media_type}" --data-binary "@${candidate}/protocol-probe.bin" http://localhost/api/auth/login)
[[ ${protocol_status} == 400 ]] || { echo "candidate protocol check returned HTTP ${protocol_status}" >&2; exit 1; }
rm -f "${maintenance}"
maintenance_created=false
chmod 0750 /run/bokheim
traffic_open=true
systemctl daemon-reload
api_status=$(curl --silent --output /dev/null --write-out '%{http_code}' --max-time 10 https://app.bokheim.se/api/account/snapshot)
[[ ${api_status} == 401 ]] || { echo "public API verification returned HTTP ${api_status}" >&2; exit 1; }
rm -rf -- "${incoming}"
echo "Bokheim sync server deployed. Full database and previous release retained in ${backup}."

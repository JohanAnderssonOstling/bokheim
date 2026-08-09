#!/usr/bin/env bash
set -euo pipefail

source_root=${1:-/home/johan/bokheim-deploy/source}
if [[ ${EUID} -ne 0 ]]; then
    echo "run this installer as root: sudo $0 [staged-source-root]" >&2
    exit 2
fi
if [[ ! ${source_root} = /* ]]; then
    echo "staged source root must be an absolute path" >&2
    exit 2
fi

sync_binary=${source_root}/deploy-target/release/sync-server
catalog_binary=${source_root}/deploy-target/release/bokheim-catalog-server
catalog_file=${source_root}/Catalogs/opds.json
for required in "${sync_binary}" "${catalog_binary}" "${catalog_file}" "${source_root}/Server/deploy/bokheim-sync.service" "${source_root}/Server/deploy/bokheim-gc.service" "${source_root}/Server/deploy/bokheim-gc.timer" "${source_root}/Server/deploy/bokheim-tmpfiles.conf" "${source_root}/Catalogs/deploy/bokheim-catalogs.service" "${source_root}/Server/deploy/Caddyfile"; do
    if [[ ! -f ${required} ]]; then
        echo "missing staged deployment file: ${required}" >&2
        exit 1
    fi
done
if [[ ! -x ${sync_binary} || ! -x ${catalog_binary} ]]; then
    echo "staged release binaries must be executable" >&2
    exit 1
fi
"${catalog_binary}" --check-catalog "${catalog_file}"

bootstrap_password_file=/etc/bokheim/admin-password
bootstrap_drop_in=/etc/systemd/system/bokheim-sync.service.d/bootstrap-admin.conf
admin_password=

export DEBIAN_FRONTEND=noninteractive
apt-get update
apt-get install -y postgresql postgresql-client ca-certificates curl gpg debian-keyring debian-archive-keyring apt-transport-https util-linux

caddy_repository_tmp=$(mktemp -d)
cleanup() {
    rm -rf -- "${caddy_repository_tmp}"
}
trap cleanup EXIT
curl --proto '=https' --tlsv1.2 --fail --silent --show-error --location https://dl.cloudsmith.io/public/caddy/stable/gpg.key --output "${caddy_repository_tmp}/caddy.gpg.key"
gpg --batch --yes --dearmor --output /usr/share/keyrings/caddy-stable-archive-keyring.gpg "${caddy_repository_tmp}/caddy.gpg.key"
curl --proto '=https' --tlsv1.2 --fail --silent --show-error --location https://dl.cloudsmith.io/public/caddy/stable/debian.deb.txt --output "${caddy_repository_tmp}/caddy-stable.list"
install -o root -g root -m 0644 "${caddy_repository_tmp}/caddy-stable.list" /etc/apt/sources.list.d/caddy-stable.list
chmod 0644 /usr/share/keyrings/caddy-stable-archive-keyring.gpg
apt-get update
apt-get install -y caddy

if ! getent group bokheim >/dev/null; then
    groupadd --system bokheim
fi
if ! id -u bokheim >/dev/null 2>&1; then
    useradd --system --gid bokheim --home-dir /var/lib/bokheim --shell /usr/sbin/nologin bokheim
fi

# The large disk also serves existing applications. Sticky world-writable mode
# preserves their access while preventing unprivileged users from renaming or
# deleting Bokheim's protected child directory.
chmod 1777 /home/johan/data
install -d -o bokheim -g bokheim -m 0750 /home/johan/data/bokheim
install -d -o bokheim -g bokheim -m 0750 /var/lib/bokheim /var/lib/bokheim/assets

fstab_line='/home/johan/data/bokheim /var/lib/bokheim/assets none bind,nosuid,nodev,noexec 0 0'
if grep -Eq '^[^#[:space:]]+[[:space:]]+/var/lib/bokheim/assets[[:space:]]' /etc/fstab; then
    if ! grep -Fqx "${fstab_line}" /etc/fstab; then
        echo "refusing to replace an existing, different /var/lib/bokheim/assets mount in /etc/fstab" >&2
        exit 1
    fi
else
    printf '\n%s\n' "${fstab_line}" >>/etc/fstab
fi
if ! findmnt --mountpoint /var/lib/bokheim/assets >/dev/null 2>&1; then
    mount /var/lib/bokheim/assets
fi
findmnt --mountpoint /var/lib/bokheim/assets >/dev/null
install -d -o bokheim -g bokheim -m 0750 /var/lib/bokheim/assets/books /var/lib/bokheim/assets/thumbnails

systemctl enable --now postgresql.service
install -d -o root -g root -m 0700 /etc/bokheim
database_credential=/etc/bokheim/database-url
if [[ -f ${database_credential} ]]; then
    database_url=$(<"${database_credential}")
    db_password=${database_url#postgresql://bokheim_app:}
    db_password=${db_password%%@*}
    if [[ ! ${db_password} =~ ^[[:xdigit:]]{64}$ ]]; then
        echo "existing database credential has an unexpected format" >&2
        exit 1
    fi
else
    db_password=$(openssl rand -hex 32)
    database_url="postgresql://bokheim_app:${db_password}@127.0.0.1/bokheim"
    umask 077
    printf '%s\n' "${database_url}" >"${database_credential}"
fi
chmod 0600 "${database_credential}"
chown root:root "${database_credential}"

runuser -u postgres -- psql --set=ON_ERROR_STOP=1 postgres <<SQL
DO \$\$
BEGIN
    IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'bokheim_app') THEN
        CREATE ROLE bokheim_app LOGIN PASSWORD '${db_password}';
    END IF;
END
\$\$;
ALTER ROLE bokheim_app WITH LOGIN PASSWORD '${db_password}';
SQL
unset db_password

# This is a development deployment target: every install wipes and recreates
# the database rather than preserving data across deploys, so there is no
# migration history to maintain -- `sync-server migrate` applies the current
# schema fresh each time. WITH (FORCE) drops the database even if the
# previous deploy's service is still holding connections open.
runuser -u postgres -- psql --set=ON_ERROR_STOP=1 postgres <<SQL
DROP DATABASE IF EXISTS bokheim WITH (FORCE);
CREATE DATABASE bokheim OWNER bokheim_app;
SQL
DATABASE_URL="${database_url}" "${sync_binary}" migrate
unset database_url

if [[ ! -f ${bootstrap_password_file} ]]; then
    if [[ ! -t 0 ]]; then
        echo "first installation requires an interactive terminal for the initial admin password" >&2
        exit 2
    fi
    while true; do
        IFS= read -r -s -p "Initial admin@bokheim.local password (at least 12 characters): " admin_password
        echo
        IFS= read -r -s -p "Confirm initial admin@bokheim.local password: " confirmation
        echo
        if [[ ${#admin_password} -lt 12 ]]; then
            echo "password is shorter than 12 characters" >&2
        elif [[ ${#admin_password} -gt 1024 ]]; then
            echo "password is longer than 1024 bytes" >&2
        elif [[ ${admin_password} != "${confirmation}" ]]; then
            echo "passwords do not match" >&2
        else
            unset confirmation
            break
        fi
    done
fi

install -o root -g root -m 0755 "${sync_binary}" /usr/local/bin/sync-server
install -o root -g root -m 0755 "${catalog_binary}" /usr/local/bin/bokheim-catalog-server
install -d -o root -g root -m 0755 /usr/local/share/bokheim-catalogs
install -o root -g root -m 0644 "${catalog_file}" /usr/local/share/bokheim-catalogs/opds.json

install -o root -g root -m 0644 "${source_root}/Server/deploy/bokheim-sync.service" /etc/systemd/system/bokheim-sync.service
install -o root -g root -m 0644 "${source_root}/Server/deploy/bokheim-gc.service" /etc/systemd/system/bokheim-gc.service
install -o root -g root -m 0644 "${source_root}/Server/deploy/bokheim-gc.timer" /etc/systemd/system/bokheim-gc.timer
install -d -o root -g root -m 0755 /etc/tmpfiles.d
install -o root -g root -m 0644 "${source_root}/Server/deploy/bokheim-tmpfiles.conf" /etc/tmpfiles.d/bokheim.conf
systemd-tmpfiles --create /etc/tmpfiles.d/bokheim.conf
install -o root -g root -m 0644 "${source_root}/Catalogs/deploy/bokheim-catalogs.service" /etc/systemd/system/bokheim-catalogs.service
install -d -o root -g root -m 0755 /etc/systemd/system/caddy.service.d /etc/systemd/system/bokheim-sync.service.d
install -o root -g root -m 0644 "${source_root}/Server/deploy/caddy-bokheim.conf" /etc/systemd/system/caddy.service.d/bokheim.conf
install -o root -g root -m 0644 "${source_root}/Server/deploy/Caddyfile" /etc/caddy/Caddyfile
install -o root -g root -m 0600 "${source_root}/Server/deploy/caddy.env.example" /etc/bokheim/caddy.env
install -o root -g root -m 0755 "${source_root}/Server/deploy/verify-public.sh" /usr/local/sbin/bokheim-verify-public

if [[ -n ${admin_password} ]]; then
    umask 077
    printf '%s' "${admin_password}" >"${bootstrap_password_file}"
    unset admin_password
fi
# The database is wiped on every deploy, so the admin account never survives
# a redeploy either -- the server re-bootstraps it from this credential on
# every start (see PostgresAccountService::bootstrap_admin), so both the
# credential file and this drop-in must persist rather than being cleaned up
# after a successful first install.
cat >"${bootstrap_drop_in}" <<'EOF'
[Service]
LoadCredential=admin_password:/etc/bokheim/admin-password
EOF
chmod 0644 "${bootstrap_drop_in}"

systemctl daemon-reload
caddy validate --envfile /etc/bokheim/caddy.env --config /etc/caddy/Caddyfile
systemctl enable bokheim-sync.service bokheim-catalogs.service bokheim-gc.timer caddy.service
systemctl restart bokheim-sync.service
systemctl restart bokheim-catalogs.service
systemctl restart caddy.service
systemctl start bokheim-gc.timer

for _ in $(seq 1 20); do
    if curl --fail --silent --show-error --unix-socket /run/bokheim/sync.sock http://localhost/api/ready | grep -Fqx ok; then
        sync_ready=true
        break
    fi
    sleep 1
done
if [[ ${sync_ready:-false} != true ]]; then
    echo "sync server did not become healthy; bootstrap credential was preserved for retry" >&2
    systemctl --no-pager --full status bokheim-sync.service >&2 || true
    exit 1
fi
curl --fail --silent --show-error http://127.0.0.1:8090/health | grep -Fqx ok

echo "Bokheim services installed and healthy locally."
echo "Public verification: sudo bokheim-verify-public api.bokheim.se catalogs.bokheim.se"

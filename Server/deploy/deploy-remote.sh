#!/usr/bin/env bash
set -euo pipefail

# Build and deploy a prepared Bokheim host over SSH. The remote host must
# already have the dependencies and service configuration installed by
# install-server.sh; this script performs code/data upgrades only.

script_directory=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
repository_root=$(cd -- "${script_directory}/../.." && pwd)
host=${BOKHEIM_DEPLOY_HOST:-192.168.1.68}
user=${BOKHEIM_DEPLOY_USER:-${USER}}
remote_prefix=${BOKHEIM_DEPLOY_REMOTE_PREFIX:-/tmp/bokheim-deploy}
ssh_options=()
if [[ -n ${BOKHEIM_DEPLOY_SSH_OPTS:-} ]]; then
    read -r -a ssh_options <<<"${BOKHEIM_DEPLOY_SSH_OPTS}"
fi

if [[ ${1:-} == --help || ${1:-} == -h ]]; then
    cat <<'EOF'
Usage: Server/deploy/deploy-remote.sh

Environment:
  BOKHEIM_DEPLOY_HOST          SSH host (default: 192.168.1.68)
  BOKHEIM_DEPLOY_USER          SSH user (default: current user)
  BOKHEIM_DEPLOY_SSH_OPTS      Extra SSH options, space separated
  BOKHEIM_DEPLOY_REMOTE_PREFIX Temporary directory prefix (default: /tmp/bokheim-deploy)
EOF
    exit 0
fi

if [[ ${EUID} -eq 0 ]]; then
    echo "refusing to build and deploy as root; run this from the development checkout" >&2
    exit 2
fi

remote=${user}@${host}
stage=$(mktemp -d)
remote_stage=
cleanup() {
    rm -rf -- "${stage}"
    if [[ -n ${remote_stage} ]]; then
        ssh "${ssh_options[@]}" "${remote}" "rm -rf -- '${remote_stage}'" >/dev/null 2>&1 || true
    fi
}
trap cleanup EXIT

echo "Building sync-server..."
cargo build --release --manifest-path "${repository_root}/Server/Cargo.toml" -p sync-server
echo "Building catalog-server..."
cargo build --release --manifest-path "${repository_root}/Catalogs/Cargo.toml" -p catalog-server

sync_binary=${repository_root}/Server/target/release/sync-server
catalog_binary=${repository_root}/target/release/catalog-server
for required in "${sync_binary}" "${catalog_binary}" "${repository_root}/Catalogs/opds.json"; do
    if [[ ! -x ${required} && ${required} != *.json ]]; then
        echo "missing release artifact: ${required}" >&2
        exit 1
    fi
    if [[ ! -f ${required} ]]; then
        echo "missing deployment file: ${required}" >&2
        exit 1
    fi
done

install -m 0755 "${sync_binary}" "${stage}/sync-server"
install -m 0755 "${catalog_binary}" "${stage}/catalog-server"
install -m 0644 "${repository_root}/Catalogs/opds.json" "${stage}/opds.json"
install -m 0644 "${repository_root}/Server/deploy/install-staged.sh" "${stage}/install-staged.sh"
install -m 0644 "${repository_root}/Server/deploy/bokheim-sync.service" "${stage}/bokheim-sync.service"
install -m 0644 "${repository_root}/Catalogs/deploy/catalog-providers.service" "${stage}/catalog-providers.service"

echo "Checking SSH access to ${remote}..."
remote_stage=$(ssh "${ssh_options[@]}" "${remote}" "mktemp -d '${remote_prefix}.XXXXXX'")
scp "${ssh_options[@]}" "${stage}"/* "${remote}:${remote_stage}/"

echo "Installing staged release on ${remote}..."
ssh -tt "${ssh_options[@]}" "${remote}" \
    "sudo bash '${remote_stage}/install-staged.sh' '${remote_stage}'"

echo "Deployment completed successfully on ${remote}."

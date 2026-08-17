#!/usr/bin/env bash
set -euo pipefail

# Build and deploy a prepared Bokheim host over SSH. The remote host must
# already have the dependencies and service configuration installed by
# install-server.sh; this script performs code/data upgrades only.

script_directory=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
repository_root=$(cd -- "${script_directory}/../../.." && pwd)
host=${BOKHEIM_DEPLOY_HOST:-192.168.1.68}
user=${BOKHEIM_DEPLOY_USER:-${USER}}
remote_prefix=${BOKHEIM_DEPLOY_REMOTE_PREFIX:-/var/lib/bokheim-deploy/upload}
stage_only=false
ssh_options=()
if [[ -n ${BOKHEIM_DEPLOY_SSH_OPTS:-} ]]; then
    read -r -a ssh_options <<<"${BOKHEIM_DEPLOY_SSH_OPTS}"
fi

if [[ ${1:-} == --help || ${1:-} == -h ]]; then
    cat <<'EOF'
Usage: servers/sync/deploy/deploy-remote.sh [--stage-only]

  --stage-only  Build and transfer a release without activating it.

Environment:
  BOKHEIM_DEPLOY_HOST          SSH host (default: 192.168.1.68)
  BOKHEIM_DEPLOY_USER          SSH user (default: current user)
  BOKHEIM_DEPLOY_SSH_OPTS      Extra SSH options, space separated
  BOKHEIM_DEPLOY_REMOTE_PREFIX Temporary directory prefix (default: /tmp/bokheim-deploy)
EOF
    exit 0
fi
if [[ ${1:-} == --stage-only ]]; then
    stage_only=true
elif [[ $# -ne 0 ]]; then
    echo "unknown argument: $1" >&2
    exit 2
fi

if [[ ${EUID} -eq 0 ]]; then
    echo "refusing to build and deploy as root; run this from the development checkout" >&2
    exit 2
fi

remote=${user}@${host}
local_stage_root=${BOKHEIM_DEPLOY_LOCAL_STAGE_ROOT:-${repository_root}/.deploy-tmp}
install -d -m 0700 "${local_stage_root}"
stage=$(mktemp -d "${local_stage_root}/server.XXXXXX")
remote_stage=
keep_remote_stage=false
cleanup() {
    rm -rf -- "${stage}"
    if [[ -n ${remote_stage} && ${keep_remote_stage} == false ]]; then
        ssh "${ssh_options[@]}" "${remote}" "rm -rf -- '${remote_stage}'" >/dev/null 2>&1 || true
    fi
}
trap cleanup EXIT

echo "Building sync-server..."
cargo build --release --manifest-path "${repository_root}/servers/sync/Cargo.toml" -p sync-server
echo "Building catalog-server..."
cargo build --release --manifest-path "${repository_root}/servers/opds-cache/Cargo.toml" -p catalog-server
echo "Building metadata-server..."
cargo build --release --manifest-path "${repository_root}/Cargo.toml" -p metadata-server

sync_binary=${repository_root}/servers/sync/target/release/sync-server
catalog_binary=${repository_root}/target/release/catalog-server
metadata_binary=${repository_root}/target/release/metadata-server
for required in "${sync_binary}" "${catalog_binary}" "${metadata_binary}" "${repository_root}/servers/opds-cache/opds.json" "${repository_root}/servers/sync/deploy/bokheim-sync.service" "${repository_root}/servers/opds-cache/deploy/bokheim-catalogs.service" "${repository_root}/servers/metadata/deploy/bokheim-metadata.service"; do
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
install -m 0755 "${metadata_binary}" "${stage}/metadata-server"
install -m 0644 "${repository_root}/servers/opds-cache/opds.json" "${stage}/opds.json"
install -m 0644 "${repository_root}/servers/sync/deploy/bokheim-sync.service" "${stage}/bokheim-sync.service"
install -m 0644 "${repository_root}/servers/opds-cache/deploy/bokheim-catalogs.service" "${stage}/bokheim-catalogs.service"
install -m 0644 "${repository_root}/servers/metadata/deploy/bokheim-metadata.service" "${stage}/bokheim-metadata.service"
echo "Checking SSH access to ${remote}..."
remote_stage=$(ssh "${ssh_options[@]}" "${remote}" "mktemp -d '${remote_prefix}.XXXXXX'")
scp "${ssh_options[@]}" "${stage}"/* "${remote}:${remote_stage}/"
ssh "${ssh_options[@]}" "${remote}" \
    "if test -d /var/lib/bokheim-deploy/incoming; then mv /var/lib/bokheim-deploy/incoming /var/lib/bokheim-deploy/previous.\$\$; fi; mv '${remote_stage}' /var/lib/bokheim-deploy/incoming"
remote_stage=/var/lib/bokheim-deploy/incoming

if [[ ${stage_only} == true ]]; then
    keep_remote_stage=true
    echo "Release staged on ${remote}."
    echo "Activate with: sudo /usr/local/sbin/bokheim-deploy-servers"
    exit 0
fi

echo "Installing staged release on ${remote}..."
ssh "${ssh_options[@]}" "${remote}" "sudo -n /usr/local/sbin/bokheim-deploy-servers"
keep_remote_stage=true

echo "Deployment completed successfully on ${remote}."

#!/usr/bin/env bash
set -euo pipefail

# Build and deploy a prepared Bokheim host over SSH. The remote host must
# already have the dependencies and service configuration installed by
# install-server.sh; this script performs code/data upgrades only.

script_directory=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
repository_root=$(cd -- "${script_directory}/../../.." && pwd)
host=${BOKHEIM_DEPLOY_HOST:-192.168.1.68}
user=${BOKHEIM_DEPLOY_USER:-${USER}}
remote_prefix=${BOKHEIM_DEPLOY_REMOTE_PREFIX:-/tmp/bokheim-deploy}
stage_only=false
ssh_options=()
if [[ -n ${BOKHEIM_DEPLOY_SSH_OPTS:-} ]]; then
    read -r -a ssh_options <<<"${BOKHEIM_DEPLOY_SSH_OPTS}"
fi

if [[ ${1:-} == --help || ${1:-} == -h ]]; then
    cat <<'EOF'
Usage: servers/sync/deploy/deploy-remote.sh [--stage-only]

  --stage-only  Build and transfer a release, then print the remote sudo
                command instead of running it.

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
stage=$(mktemp -d)
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

sync_binary=${repository_root}/servers/sync/target/release/sync-server
catalog_binary=${repository_root}/target/release/catalog-server
for required in "${sync_binary}" "${catalog_binary}" "${repository_root}/servers/opds-cache/opds.json"; do
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
install -m 0644 "${repository_root}/servers/opds-cache/opds.json" "${stage}/opds.json"
install -m 0644 "${repository_root}/servers/sync/deploy/install-staged.sh" "${stage}/install-staged.sh"
install -m 0755 "${repository_root}/servers/sync/deploy/deploy-sudo.sh" "${stage}/deploy-sudo.sh"
install -m 0644 "${repository_root}/servers/sync/deploy/bokheim-sync.service" "${stage}/bokheim-sync.service"
install -m 0644 "${repository_root}/servers/opds-cache/deploy/bokheim-catalogs.service" "${stage}/bokheim-catalogs.service"

echo "Checking SSH access to ${remote}..."
remote_stage=$(ssh "${ssh_options[@]}" "${remote}" "mktemp -d '${remote_prefix}.XXXXXX'")
scp "${ssh_options[@]}" "${stage}"/* "${remote}:${remote_stage}/"

if [[ ${stage_only} == true ]]; then
    keep_remote_stage=true
    echo "Release staged on ${remote}."
    echo "On the deployment host, run: sudo '${remote_stage}/deploy-sudo.sh'"
    exit 0
fi

echo "Installing staged release on ${remote}..."
ssh -tt "${ssh_options[@]}" "${remote}" \
    "sudo '${remote_stage}/deploy-sudo.sh'"

echo "Deployment completed successfully on ${remote}."

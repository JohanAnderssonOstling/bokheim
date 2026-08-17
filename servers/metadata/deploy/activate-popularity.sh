#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 4 ]]; then
    echo "usage: $0 SOURCE.sqlite RATINGS.txt.gz READING-LOG.txt.gz OUTPUT.sqlite" >&2
    exit 2
fi

source_snapshot=$1
ratings=$2
reading_log=$3
output=$4
current=/home/johan/data/openlibrary/current.sqlite
release=/var/lib/bokheim-deploy/metadata-catalog-incoming
deploy=/usr/local/sbin/bokheim-deploy-metadata-catalog

for path in "${source_snapshot}" "${ratings}" "${reading_log}" "${output}"; do
    [[ ${path} == /* ]] || { echo "paths must be absolute" >&2; exit 2; }
done
for path in "${source_snapshot}" "${ratings}" "${reading_log}"; do
    [[ -f ${path} && ! -L ${path} ]] || { echo "missing input: ${path}" >&2; exit 1; }
done
[[ ! -e ${output} && ! -e ${output}.building ]] || { echo "output already exists: ${output}" >&2; exit 1; }
for required in metadata-server catalog-server opds.json; do
    [[ -f ${release}/${required} && ! -L ${release}/${required} ]] || { echo "missing staged release artifact: ${required}" >&2; exit 1; }
done

"${release}/metadata-server" add-popularity "${source_snapshot}" "${ratings}" "${reading_log}" "${output}"

schema=$(sqlite3 -readonly "${output}" 'SELECT schema_version FROM snapshot WHERE singleton=1;')
popularity=$(sqlite3 -readonly "${output}" 'SELECT COUNT(*) FROM work_popularity;')
entities=$(sqlite3 -readonly "${output}" 'SELECT COUNT(*) FROM entity;')
integrity=$(sqlite3 -readonly "${output}" 'PRAGMA integrity_check;')
[[ ${schema} == 8 ]] || { echo "expected schema 8, got ${schema}" >&2; exit 1; }
[[ ${popularity} =~ ^[1-9][0-9]*$ ]] || { echo "snapshot contains no popularity aggregates" >&2; exit 1; }
[[ ${entities} == 0 ]] || { echo "schema-v8 snapshot unexpectedly contains resolved Wikidata entities" >&2; exit 1; }
[[ ${integrity} == ok ]] || { echo "snapshot integrity check failed: ${integrity}" >&2; exit 1; }

verify_log=${output}.verify.log
BOKHEIM_METADATA_DATABASE=${output} BOKHEIM_METADATA_BIND=127.0.0.1:18091 \
    "${release}/metadata-server" >"${verify_log}" 2>&1 &
verify_pid=$!
cleanup() {
    kill "${verify_pid}" 2>/dev/null || true
    wait "${verify_pid}" 2>/dev/null || true
}
trap cleanup EXIT
ready=false
for _ in $(seq 1 30); do
    if curl --fail --silent --max-time 2 http://127.0.0.1:18091/health | grep -Fqx ok; then
        ready=true
        break
    fi
    sleep 1
done
[[ ${ready} == true ]] || { echo "staged metadata server could not open schema-v8 snapshot" >&2; exit 1; }
curl --fail --silent --show-error --max-time 10 \
    -H 'Content-Type: application/json' \
    --data '{"isbns":["9780306406157"]}' \
    http://127.0.0.1:18091/v2/enrichment >/dev/null
cleanup
trap - EXIT

previous=$(readlink -f "${current}" 2>/dev/null || true)
ln -sfn "${output}" "${current}.next"
mv -Tf "${current}.next" "${current}"
if ! sudo -n "${deploy}"; then
    if [[ -n ${previous} ]]; then
        ln -sfn "${previous}" "${current}.next"
        mv -Tf "${current}.next" "${current}"
    fi
    echo "combined deployment failed; restored the previous metadata snapshot link" >&2
    exit 1
fi
echo "Open Library and popularity schema-v8 snapshot activated; schema-v9 Wikidata build was not interrupted."

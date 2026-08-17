#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 3 ]]; then
    echo "usage: $0 SOURCE.sqlite DATA_DIRECTORY OUTPUT.sqlite" >&2
    exit 2
fi

source_snapshot=$1
data_directory=$2
output=$3
script_directory=$(cd -- "$(dirname -- "$0")" && pwd)
ratings=${data_directory}/ol_dump_ratings_latest.txt.gz
reading_log=${data_directory}/ol_dump_reading-log_latest.txt.gz

mkdir -p "${data_directory}"

download_until_complete() {
    url=$1
    destination=$2
    log=$3
    while ! curl --fail --location --continue-at - --connect-timeout 20 --output "${destination}" "${url}" >>"${log}" 2>&1; do
        sleep 60
    done
    gzip -t "${destination}"
}

download_until_complete https://openlibrary.org/data/ol_dump_ratings_latest.txt.gz "${ratings}" "${data_directory}/ratings-download.log" &
ratings_pid=$!
download_until_complete https://openlibrary.org/data/ol_dump_reading-log_latest.txt.gz "${reading_log}" "${data_directory}/reading-log-download.log" &
reading_pid=$!
wait "${ratings_pid}"
wait "${reading_pid}"

exec "${script_directory}/activate-popularity.sh" "${source_snapshot}" "${ratings}" "${reading_log}" "${output}"

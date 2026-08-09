#!/usr/bin/env bash
set -euo pipefail

script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
repo_root=$(cd -- "$script_dir/../.." && pwd)
temp_root=${TMPDIR:-/tmp}
e2e_tmp=$(mktemp -d "$temp_root/bokheim-sync-e2e.XXXXXX")
postgres_data="$e2e_tmp/postgres"
postgres_socket=$(mktemp -d /tmp/bokheim-sync-socket.XXXXXX)
postgres_started=0
server_pid=

cleanup() {
    local exit_code=$?
    if [[ -n "$server_pid" ]]; then
        kill "$server_pid" >/dev/null 2>&1 || true
        wait "$server_pid" >/dev/null 2>&1 || true
    fi
    if [[ "$postgres_started" == 1 ]]; then
        pg_ctl -D "$postgres_data" -m immediate -w stop >/dev/null 2>&1 || true
    fi
    if [[ "$exit_code" != 0 && -f "$e2e_tmp/server.log" ]]; then
        sed -n '1,240p' "$e2e_tmp/server.log" >&2
    fi
    if [[ "$e2e_tmp" == "$temp_root"/bokheim-sync-e2e.* ]]; then
        rm -rf -- "$e2e_tmp"
    fi
    if [[ "$postgres_socket" == /tmp/bokheim-sync-socket.* ]]; then
        rm -rf -- "$postgres_socket"
    fi
}
trap cleanup EXIT

initdb -D "$postgres_data" -A trust -U postgres --no-locale >/dev/null

postgres_port=$((55000 + ($$ % 5000)))
while (echo >/dev/tcp/127.0.0.1/"$postgres_port") >/dev/null 2>&1; do
    postgres_port=$((postgres_port + 1))
done

if ! pg_ctl \
    -D "$postgres_data" \
    -l "$e2e_tmp/postgres.log" \
    -o "-F -h 127.0.0.1 -p $postgres_port -k $postgres_socket" \
    -w start >/dev/null; then
    sed -n '1,200p' "$e2e_tmp/postgres.log" >&2
    exit 1
fi
postgres_started=1

database_name="sync_e2e_$$"
createdb -h 127.0.0.1 -p "$postgres_port" -U postgres "$database_name"

server_target=${SYNC_E2E_SERVER_TARGET_DIR:-$repo_root/Server/target}
backend_target=${SYNC_E2E_BACKEND_TARGET_DIR:-$repo_root/target}
CARGO_TARGET_DIR="$server_target" cargo build --manifest-path "$repo_root/Server/Cargo.toml"

SYNC_E2E_DATABASE_URL="postgres://postgres@127.0.0.1:$postgres_port/$database_name" \
CARGO_TARGET_DIR="$server_target" \
cargo test \
    --manifest-path "$repo_root/Server/Cargo.toml" \
    -p server-account \
    -p server-postgres \
    -- \
    --ignored \
    --test-threads=1

SYNC_E2E_DATABASE_URL="postgres://postgres@127.0.0.1:$postgres_port/$database_name" \
CARGO_TARGET_DIR="$server_target" \
cargo test \
    --manifest-path "$repo_root/Server/Cargo.toml" \
    -p sync-server \
    --test public_registration \
    -- \
    --ignored \
    --test-threads=1

gc_books="$e2e_tmp/gc-books"
gc_thumbnails="$e2e_tmp/gc-thumbnails"
mkdir -p "$gc_books" "$gc_thumbnails"
DATABASE_URL="postgres://postgres@127.0.0.1:$postgres_port/$database_name" \
BOKHEIM_BOOKS_DIR="$gc_books" \
BOKHEIM_THUMBNAILS_DIR="$gc_thumbnails" \
BOKHEIM_GC_GRACE_HOURS=1 \
"$server_target/debug/sync-server" gc

server_port=$((50000 + ($$ % 5000)))
while (echo >/dev/tcp/127.0.0.1/"$server_port") >/dev/null 2>&1; do
    server_port=$((server_port + 1))
done
DATABASE_URL="postgres://postgres@127.0.0.1:$postgres_port/$database_name" \
SYNC_SERVER_ADDR="127.0.0.1:$server_port" \
BOKHEIM_ADMIN_PASSWORD="sync-e2e-admin-password" \
BOKHEIM_BOOKS_DIR="$gc_books" \
BOKHEIM_THUMBNAILS_DIR="$gc_thumbnails" \
"$server_target/debug/sync-server" >"$e2e_tmp/server.log" 2>&1 &
server_pid=$!
for _ in $(seq 1 100); do
    if curl --fail --silent "http://127.0.0.1:$server_port/api/ready" >/dev/null; then
        break
    fi
    sleep 0.1
done
curl --fail --silent "http://127.0.0.1:$server_port/api/ready" >/dev/null
curl --fail --silent "http://127.0.0.1:$server_port/api/live" >/dev/null
curl --fail --silent "http://127.0.0.1:$server_port/api/health" >/dev/null

SYNC_E2E_DATABASE_URL="postgres://postgres@127.0.0.1:$postgres_port/$database_name" \
SYNC_E2E_SERVER_BIN="$server_target/debug/sync-server" \
SYNC_E2E_SERVER_URL="http://127.0.0.1:$server_port" \
SYNC_E2E_ADMIN_PASSWORD="sync-e2e-admin-password" \
CARGO_TARGET_DIR="$backend_target" \
cargo test \
    --manifest-path "$repo_root/Backend/Cargo.toml" \
    --lib \
    sync::manager::tests::production_http_postgres_ \
    -- \
    --ignored \
    --test-threads=1

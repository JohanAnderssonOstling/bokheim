#!/usr/bin/env bash
set -euo pipefail

script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
repo_root=$(cd -- "$script_dir/../.." && pwd)
temp_root=${TMPDIR:-/tmp}
e2e_tmp=$(mktemp -d "$temp_root/bokheim-sync-e2e.XXXXXX")
postgres_data="$e2e_tmp/postgres"
postgres_socket=$(mktemp -d /tmp/bokheim-sync-socket.XXXXXX)
postgres_started=0

cleanup() {
    if [[ "$postgres_started" == 1 ]]; then
        pg_ctl -D "$postgres_data" -m immediate -w stop >/dev/null 2>&1 || true
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
    -p server-account-postgres \
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

SYNC_E2E_DATABASE_URL="postgres://postgres@127.0.0.1:$postgres_port/$database_name" \
SYNC_E2E_SERVER_BIN="$server_target/debug/sync-server" \
CARGO_TARGET_DIR="$backend_target" \
cargo test \
    --manifest-path "$repo_root/Backend/Cargo.toml" \
    --lib \
    app_core::tests::production_http_postgres_ \
    -- \
    --ignored \
    --test-threads=1

SYNC_E2E_DATABASE_URL="postgres://postgres@127.0.0.1:$postgres_port/$database_name" \
SYNC_E2E_SERVER_BIN="$server_target/debug/sync-server" \
CARGO_TARGET_DIR="$backend_target" \
cargo test \
    --manifest-path "$repo_root/Backend/Cargo.toml" \
    --lib \
    sync::tests::production_http_postgres_ \
    -- \
    --ignored \
    --test-threads=1

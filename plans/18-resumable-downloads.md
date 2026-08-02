# Plan 18: Resumable blob downloads

Atomic; touches `Backend/src/transfer.rs::download_blob` + the server blob GET route.

## Problem

A connection drop mid-download deletes the tmp file and restarts from byte 0 on retry. Fine on
wifi, hostile on mobile.

## Change

1. Server blob route: support `Range: bytes=N-` (the blob is a static file/object; axum/tower
   or manual `Range` handling), respond `206` with the tail.
2. Client `download_blob`: on start, if `{local_path}.tmp` exists, hash the existing prefix
   with blake3 (streaming) and request `Range: bytes=<len>-`; append. On `200` (server ignores
   Range), truncate and restart. Keep the existing full-file hash verification at the end —
   it already guards corrupt resumes; on mismatch delete tmp and retry from scratch (existing
   path).
3. Keep tmp file on transfer error (delete only on hash mismatch or completion) — currently it
   is deleted on every chunk error, which is what defeats resumption.

## Tests

1. Server: Range request returns the correct tail; no-Range unchanged.
2. Client: kill a mocked stream mid-transfer, re-run job → request carries `Range`, final file
   hash-verifies; corrupted tmp prefix → mismatch → clean restart succeeds.

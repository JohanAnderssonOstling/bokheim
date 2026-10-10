# Asset transfer

Executes typed book and cover operations on the library thread. The canonical
orchestration design is `docs/asset-loop-design.html` at the repository root.

- `transfer::AssetExecutor`: bounded manifest negotiation, credential renewal,
  cover presence validation, native book uploads with concurrency four, browser
  uploads sequentially, and rejected cover batch isolation.
- `books`: verified uploads and native downloads, checksums, guarded revision
  completion, conditional placement, and browser Blob uploads.
- `thumbnails`: cover upload/download batches, response validation, local variant
  recovery and partial batch commits. Hosts supply resizing and notifications.
- `thumbnail_jobs`: local cover preparation using the host CPU capability.
- `policy` and `deadline`: transfer time limits and cancellation cleanup.
- `TransferError`: authentication, retryable and rejected outcomes.

The backend owns the asset pass and its retry schedule. Pending work comes from
bounded database pages; this crate owns no job queue, driver, history or retry
holds. Upload negotiation commits one batch; individual book transfers commit
as they finish. Cover transfers commit successfully stored items even if later
items in the response fail. Database methods recheck the captured versions and
placements, and no transaction spans a network await.

Native cover pair writes use one exclusive lease across both variants, retained
through blocking I/O if the caller cancels. Browser imports retain OPFS book
bytes; books without local bytes are streamed instead of downloaded.

Release tests:

```sh
cargo test --release -p asset-transfer --lib
cargo test --release -p library-backend --lib sync::asset_sync::tests
```

Pass tests cover phase order, same-pass cover eligibility, page continuation,
quota retries, restart recovery, cover discovery for locally held books,
authentication stops and downloads requested during an upload phase.

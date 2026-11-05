# Browser parser worker

This package builds the isolated synchronous parser and PDFium module used by
`cpu_worker_loader.js`. Book readers supply ranges from the storage owner.

The application backend is part of the UI's Wasm bundle. It runs on a shared
memory worker using typed Rust channels, with the same library, account,
management, and sync operations as native. A Web Lock admits one page at a time.
Browser file capabilities stay in the platform adapter; selected files are
cloned to their owner once and released with the import's Rust lease.

Build the application from the workspace root:

```sh
./apps/web-gpui/scripts/build.sh
```

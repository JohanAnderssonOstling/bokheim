# Bokheim GPUI for Web

This is the browser build of the same GPUI application used by the native
targets. It renders GPUI into a browser canvas and keeps browser-specific
storage behind the SQLite OPFS VFS used by the shared Rust backend.

Build the release bundle with:

```sh
./apps/web-gpui/scripts/build.sh
```

Then serve `apps/web-gpui/dist` with cross-origin isolation headers enabled.
For local development, the included server configures the required headers:

```sh
python3 apps/web-gpui/scripts/serve.py
```

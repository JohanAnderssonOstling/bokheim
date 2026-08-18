# Bokheim Mobile

The mobile application uses Tauri 2, Svelte 5, TypeScript, Vite, and a Bokheim-maintained copy of Foliate.js under `src/vendor/foliate-js`. It can open EPUBs from the Rust library backend or directly from the device file picker. Reading locations are restored and saved as EPUB CFI values.

The Foliate.js sources are committed directly to this repository, including Bokheim's local HTML, Web Publication, and PDF adaptations. No separate submodule checkout is required.

From this directory, use `npm run check` for frontend diagnostics, `npm run build` for the production web bundle, and `npm run tauri -- android build --debug` for an Android build when the Android SDK and NDK are configured.

Tauri commands, their Rust request/response types, and shared frontend constants are exported to `src/lib/bindings.ts` by tauri-specta. Run `npm run generate:bindings` after changing the command boundary, or `npm run check:bindings` to fail when the committed bindings are stale. Desktop debug runs also refresh the file automatically.

## Recommended IDE Setup

[VS Code](https://code.visualstudio.com/) + [Svelte](https://marketplace.visualstudio.com/items?itemName=svelte.svelte-vscode) + [Tauri](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode) + [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer).

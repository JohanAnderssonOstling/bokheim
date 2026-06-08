# Phase 1 — Tauri 2 + Svelte scaffold

## Goal
Stand up a buildable Tauri 2 mobile project that links the existing `Backend/` crate, runs on Android emulator, and shows a "hello" Svelte page.

## Steps

### 1.1 Create the project
At repo root, run `npm create tauri-app@latest MobileApp` and pick: TypeScript, Svelte, npm. This produces:
```
MobileApp/
  src/                  # Svelte 5 + Vite
  src-tauri/
    Cargo.toml
    tauri.conf.json
    src/lib.rs          # mobile entry: #[cfg_attr(mobile, tauri::mobile_entry_point)]
```

### 1.2 Wire backend crate
In `MobileApp/src-tauri/Cargo.toml`:
- Add `epub_svelte_lib = { path = "../../Backend" }`.
- Tauri 2 mobile requires the lib crate-type to include `cdylib` and `staticlib`. Update `Backend/Cargo.toml` `[lib]` to `crate-type = ["rlib", "cdylib", "staticlib"]`. Verify the Floem `UI/` crate (which depends on `epub_svelte_lib` as `rlib`) still builds after this change — the additional crate types are additive and should not break the rlib consumer.

### 1.3 Initialize mobile target
```
cd MobileApp
npm run tauri android init
```
This creates `src-tauri/gen/android/`. Requires: Android SDK, NDK, `ANDROID_HOME`, `NDK_HOME`, Java 17, and Rust targets `aarch64-linux-android`, `armv7-linux-androideabi`, `x86_64-linux-android`, `i686-linux-android` (`rustup target add ...`).

### 1.4 Pick Tauri plugins (install now, configure later)
- `tauri-plugin-fs` — scoped fs, SAF-aware on Android
- `tauri-plugin-dialog` — folder picker (wraps SAF on Android)
- `tauri-plugin-sql` — *not used*; we use `rusqlite` directly in backend
- `tauri-plugin-os` — for back-button / platform detection
- `tauri-plugin-log` — bridge `log` crate to Android logcat

### 1.5 Install Svelte deps
```
npm i -D svelte@5 vite @sveltejs/vite-plugin-svelte typescript
npm i svelte-spa-router        # tiny client router
```

### 1.6 Smoke test
- `npm run tauri android dev` boots emulator, loads the default page.
- Verify `log::info!` from a stub `#[tauri::command]` reaches logcat (`adb logcat | grep RustStdoutStderr`).

## Verification
- `npm run tauri android dev` builds and launches on emulator.
- Default Tauri "Greet" command works end-to-end.
- `Backend/` crate compiles for `aarch64-linux-android` (no SAF code yet; just confirms the toolchain).

## Out of scope for this phase
- No real backend commands wired (Phase 3).
- No UI navigation (Phase 4).
- No reader (Phase 5).
- No release signing (Phase 6).

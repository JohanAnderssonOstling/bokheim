# Phase 6 — Android packaging & polish

## Goal
Ship a signed APK / AAB. Handle hardware back, safe areas, permissions, app-startup edge cases, and basic perf tuning.

## 6.1 Permissions and manifest
`MobileApp/src-tauri/gen/android/app/src/main/AndroidManifest.xml`:
- `INTERNET` — for sync/transfer.
- **No storage permissions needed.** All library storage is app-private (`{app_data}/libraries/`); no `READ_EXTERNAL_STORAGE`, no `MANAGE_EXTERNAL_STORAGE`, no SAF tree URIs.
- The single-file import flow (`ACTION_OPEN_DOCUMENT`) doesn't require a manifest permission either — the system picker grants per-file read implicitly.
- `FOREGROUND_SERVICE` — only if you implement WorkManager-backed sync (optional; defer).

## 6.2 Hardware back
Confirm the back-handler from Phase 4 actually intercepts. Tauri 2 mobile fires a webview navigation when Android back is pressed; if `popstate`-based handling proves unreliable, drop down to a small Kotlin patch in `MainActivity.kt` overriding `onBackPressed()` and emitting a Tauri event.

## 6.3 Safe areas
Set `viewport-fit=cover` in `index.html`. Use `env(safe-area-inset-top/bottom)` in CSS for app-bar and bottom tabs. Test on a notched device profile.

## 6.4 Splash & icons
- Generate icons with `npm run tauri icon path/to/icon.png` (covers Android adaptive icons).
- Splash: configure in `tauri.conf.json` `app.windows[0]` or via Android theme.

## 6.5 First-run flow
On boot, frontend calls `get_libraries`:
- If empty → show "Create your first library" screen → user enters a name → `create_library(name, None)` → optional sync-source setup → home screen.
- Otherwise → navigate to last-used library (persist last-used id in `localStorage`).

No folder picker, no permission prompt. Books arrive via sync (server-driven) or single-file import.

## 6.6 Scan UX (replacing notify watcher)
Since Phase 2 dropped filesystem watching on Android:
- "Pull to refresh" on the library home tab → `invoke('scan_library')`.
- After `import_book` or `sync_once` completes, automatically run `scan_library` for that library.
- Show progress via Tauri events emitted from `scanner.rs`.

## 6.7 Performance
- Test on a low-end device profile (2GB RAM emulator).
- Lazy-load thumbnails: use `loading="lazy"` on `<img>`; backend already produces resized thumbs via `fast_image_resize`.
- Avoid loading entire EPUB to memory on open — switch reader to streaming via custom URI scheme (Phase 5 path 3) if memory issues appear.
- Disable Tauri devtools in release: `tauri.conf.json` → `app.withGlobalTauri = false`, build with `--release`.

## 6.8 Logging
- `tauri-plugin-log` configured to write to logcat in debug, file in release.
- `log::error!` for backend errors must surface to UI as toasts — wire a Tauri event channel in Phase 3 if not already done.

## 6.9 Signing & release
- Generate keystore: `keytool -genkey -v -keystore release.jks -alias upload -keyalg RSA -keysize 2048 -validity 10000`.
- Configure `MobileApp/src-tauri/gen/android/app/build.gradle.kts` `signingConfigs.release`.
- `npm run tauri android build -- --apk` → unsigned/signed APK.
- `--aab` for Play Store.
- Increment `versionCode` per release.

## 6.10 CI (optional)
GitHub Actions matrix: `ubuntu-latest` with Android NDK setup → `cargo check --target aarch64-linux-android` for Backend, `npm run tauri android build` for full APK on tags.

## Verification (end-to-end smoke)
1. Fresh install on Android emulator (API 34).
2. Create first library by name (no folder picker, no permission prompt).
3. Import 5+ EPUBs via single-file picker, or pull from a sync server.
4. Library scans automatically; books appear with thumbnails.
5. Open book → reads, back works, CFI restored on reopen.
6. Create a second library, import a book into it, switch A→B→A: tab and folder state preserved.
7. Hardware back: returns through page stack; exits app at root.
8. Kill + relaunch app: libraries listed, last-used opens, all books still present.
9. Release-mode APK install: same flow, no devtools.

## Open questions to revisit
- iOS port: same Tauri command layer; same app-private storage model (`NSDocumentDirectory`); single-file import via `UIDocumentPickerViewController`.
- WorkManager for background sync: skip in v1, add later if users ask.
- Annotations/highlights: build on top of foliate-js's annotation events, store in backend DB (new tables).
- SAF-backed "point at existing folder" mode: deferred. If real users ask for it, reintroduce the `LibraryFs` trait (Phase 2.6 of the prior plan revision) and add a `pick_directory` Tauri command. Library entries gain a `kind: 'managed' | 'saf'` discriminator.

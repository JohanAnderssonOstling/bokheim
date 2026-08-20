import { isTauri } from "@tauri-apps/api/core";

/**
 * Which backend this bundle was built against, decided by `BOKHEIM_BACKEND`
 * at build time rather than by sniffing the host at runtime.
 *
 * A constant is what lets the native bundle drop the browser backend
 * altogether: `isWebApp` folds to `false`, so the SQLite worker, its WASM, and
 * every web-only branch are unreachable and never emitted. A runtime check
 * would keep all of it in the APK for code that can never run there.
 */
export const isNativeApp = __BOKHEIM_NATIVE__;
export const isWebApp = !isNativeApp;

/**
 * The build flag and the real host must agree. They can only diverge by
 * mistake — a native bundle opened in a plain browser, or a web bundle served
 * inside the WebView — which leaves every command routed at a backend that is
 * not there. Report it once at startup: the commands themselves still fail
 * loudly, but their errors do not explain which build is wrong.
 */
export function reportBackendHostMismatch(): void {
  if (isNativeApp === isTauri()) return;
  console.error(
    isNativeApp
      ? "This bundle was built for the native backend but is running outside Tauri. Rebuild with BOKHEIM_BACKEND=web."
      : "This bundle was built for the browser backend but is running inside Tauri. Rebuild with BOKHEIM_BACKEND=native.",
  );
}

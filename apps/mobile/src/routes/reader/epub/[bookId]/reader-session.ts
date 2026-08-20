import { getCurrentWindow } from "@tauri-apps/api/window";
import { isNativeApp } from "../../../../lib/app/platform";
import {
  enableReaderImmersiveMode,
  enableVolumePageTurning,
  onVolumePageTurn,
} from "../../../../lib/reader/volume-navigation";

export interface ReaderSessionOptions {
  /// Persists reading position and preferences before the app can be killed.
  flush: () => void;
  onPageTurn: (direction: "next" | "previous") => void;
  /// The native window's close button, which must save before it closes.
  onCloseRequested: () => void;
}

/**
 * Holds the reader's session-level concerns for as long as it is open.
 *
 * Android may kill the app without warning once it is backgrounded, so every
 * event that signals "about to lose the foreground" flushes. Coming back
 * re-arms volume paging and immersive mode, which the system drops when another
 * app takes over.
 */
export function attachReaderSession(options: ReaderSessionOptions) {
  let disposed = false;
  let unlistenCloseRequested: (() => void) | undefined;

  const unlistenVolumePageTurn = onVolumePageTurn(options.onPageTurn);
  enableVolumePageTurning(true);
  enableReaderImmersiveMode(true);

  function claimSystemNavigation() {
    enableVolumePageTurning(true);
    enableReaderImmersiveMode(true);
  }

  const handleVisibilityChange = () => {
    if (document.visibilityState === "hidden") options.flush();
    else claimSystemNavigation();
  };
  const handlePageHide = () => options.flush();

  window.addEventListener("focus", claimSystemNavigation);
  window.addEventListener("pageshow", claimSystemNavigation);
  window.addEventListener("pagehide", handlePageHide);
  document.addEventListener("visibilitychange", handleVisibilityChange);

  if (isNativeApp) {
    void getCurrentWindow()
      .onCloseRequested((event) => {
        event.preventDefault();
        options.onCloseRequested();
      })
      .then((unlisten) => {
        if (disposed) unlisten();
        else unlistenCloseRequested = unlisten;
      })
      .catch((reason) => {
        console.error("Could not register the native reader window lifecycle", reason);
      });
  }

  return function dispose() {
    disposed = true;
    enableVolumePageTurning(false);
    enableReaderImmersiveMode(false);
    unlistenVolumePageTurn();
    unlistenCloseRequested?.();
    window.removeEventListener("focus", claimSystemNavigation);
    window.removeEventListener("pageshow", claimSystemNavigation);
    window.removeEventListener("pagehide", handlePageHide);
    document.removeEventListener("visibilitychange", handleVisibilityChange);
  };
}

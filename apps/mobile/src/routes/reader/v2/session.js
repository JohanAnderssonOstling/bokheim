import {
  enableReaderImmersiveMode,
  enableVolumePageTurning,
  onVolumePageTurn,
} from "../../../lib/reader/volume-navigation";

/**
 * Holds the reader's session-level concerns for as long as it is open.
 *
 * Android drops volume paging and immersive mode when another app takes the
 * foreground, so every event that means "we have it back" re-claims both.
 *
 * There is deliberately no flush here. The reader writes its position on every
 * page turn and its settings on every change, so being killed without warning
 * costs at most the page currently being read — there is nothing pending for a
 * lifecycle hook to save.
 */
export function attachSession(onPageTurn) {
  let unlistenVolume = onVolumePageTurn(onPageTurn);

  function claim() {
    enableVolumePageTurning(true);
    enableReaderImmersiveMode(true);
  }

  function claimWhenVisible() {
    if (document.visibilityState !== "hidden") claim();
  }

  claim();
  window.addEventListener("focus", claim);
  window.addEventListener("pageshow", claim);
  document.addEventListener("visibilitychange", claimWhenVisible);

  return function detach() {
    enableVolumePageTurning(false);
    enableReaderImmersiveMode(false);
    unlistenVolume();
    window.removeEventListener("focus", claim);
    window.removeEventListener("pageshow", claim);
    document.removeEventListener("visibilitychange", claimWhenVisible);
  };
}

export type VolumePageDirection = "previous" | "next";

import { APP_NAME_LOWER } from "./application-name";

declare global {
  interface Window {
    BokheimAndroid?: {
      setVolumePageTurningEnabled: (enabled: boolean) => void;
      setReaderImmersiveEnabled?: (enabled: boolean) => void;
      pickEpub?: (requestId: string) => void;
      isNativeAudiobookAvailable?: () => boolean;
      getAudiobookState?: () => string;
      openAudiobook?: (key: string, libraryId: string, contentHash: string, sourcePath: string, title: string, author: string, initialPositionMs: number) => void;
      toggleAudiobookPlayback?: () => void;
      seekAudiobookTo?: (positionMs: number) => void;
      seekAudiobookBy?: (offsetMs: number) => void;
      setAudiobookPlaybackRate?: (rate: number) => void;
      closeAudiobook?: () => void;
      isIgnoringBatteryOptimizations?: () => boolean;
      openBatteryOptimizationSettings?: () => void;
      isApkUpdaterAvailable?: () => boolean;
      checkForApkUpdate?: (force: boolean) => void;
      downloadAndInstallApk?: (url: string, sha256: string, fileName: string) => void;
      openApkInstallSettings?: () => void;
    };
  }
}

const EVENT_NAME = `${APP_NAME_LOWER}-volume-page`;

export function enableVolumePageTurning(enabled: boolean) {
  window.BokheimAndroid?.setVolumePageTurningEnabled(enabled);
}

export function enableReaderImmersiveMode(enabled: boolean) {
  window.BokheimAndroid?.setReaderImmersiveEnabled?.(enabled);
}

export function onVolumePageTurn(handler: (direction: VolumePageDirection) => void) {
  const listener = (event: Event) => {
    const direction = (event as CustomEvent<unknown>).detail;
    if (direction === "previous" || direction === "next") handler(direction);
  };
  window.addEventListener(EVENT_NAME, listener);
  return () => window.removeEventListener(EVENT_NAME, listener);
}

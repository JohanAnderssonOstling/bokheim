export type VolumePageDirection = "previous" | "next";

import { APP_NAME_LOWER } from "./application-name";

declare global {
  interface Window {
    BokheimAndroid?: {
      setVolumePageTurningEnabled: (enabled: boolean) => void;
      setReaderImmersiveEnabled?: (enabled: boolean) => void;
      pickEpub?: (requestId: string) => void;
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

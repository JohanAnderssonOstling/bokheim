import { APP_NAME_LOWER } from "../app/application-name";
import type { MobileAudiobookSession } from "./audiobook";

export type NativeAudiobookState = {
  available: boolean;
  active: boolean;
  key: string | null;
  libraryId: string | null;
  contentHash: string | null;
  sourcePath: string | null;
  title: string | null;
  author: string | null;
  positionMs: number;
  durationMs: number | null;
  playing: boolean;
  playbackRate: number;
  error: string | null;
};

const STATE_EVENT = `${APP_NAME_LOWER}-audiobook-state`;
const EMPTY_STATE: NativeAudiobookState = {
  available: false,
  active: false,
  key: null,
  libraryId: null,
  contentHash: null,
  sourcePath: null,
  title: null,
  author: null,
  positionMs: 0,
  durationMs: null,
  playing: false,
  playbackRate: 1,
  error: null,
};

function normalizeState(value: unknown): NativeAudiobookState {
  if (!value || typeof value !== "object") return EMPTY_STATE;
  const state = value as Partial<NativeAudiobookState>;
  return {
    available: state.available === true,
    active: state.active === true,
    key: typeof state.key === "string" ? state.key : null,
    libraryId: typeof state.libraryId === "string" ? state.libraryId : null,
    contentHash: typeof state.contentHash === "string" ? state.contentHash : null,
    sourcePath: typeof state.sourcePath === "string" ? state.sourcePath : null,
    title: typeof state.title === "string" ? state.title : null,
    author: typeof state.author === "string" ? state.author : null,
    positionMs:
      typeof state.positionMs === "number" && Number.isFinite(state.positionMs)
        ? Math.max(0, state.positionMs)
        : 0,
    durationMs:
      typeof state.durationMs === "number" &&
      Number.isFinite(state.durationMs) &&
      state.durationMs >= 0
        ? state.durationMs
        : null,
    playing: state.playing === true,
    playbackRate:
      typeof state.playbackRate === "number" && Number.isFinite(state.playbackRate)
        ? state.playbackRate
        : 1,
    error: typeof state.error === "string" ? state.error : null,
  };
}

export function nativeAudiobookAvailable(): boolean {
  if (typeof window === "undefined") return false;
  try {
    return window.BokheimAndroid?.isNativeAudiobookAvailable?.() === true;
  } catch {
    return false;
  }
}

export function getNativeAudiobookState(): NativeAudiobookState {
  if (typeof window === "undefined") return EMPTY_STATE;
  try {
    const serialized = window.BokheimAndroid?.getAudiobookState?.();
    return serialized ? normalizeState(JSON.parse(serialized)) : EMPTY_STATE;
  } catch {
    return EMPTY_STATE;
  }
}

export function observeNativeAudiobookState(
  handler: (state: NativeAudiobookState) => void,
) {
  const listener = (event: Event) =>
    handler(normalizeState((event as CustomEvent<unknown>).detail));
  window.addEventListener(STATE_EVENT, listener);
  handler(getNativeAudiobookState());
  return () => window.removeEventListener(STATE_EVENT, listener);
}

export function startNativeAudiobook(session: MobileAudiobookSession) {
  window.BokheimAndroid?.openAudiobook?.(
    session.nativeKey,
    session.libraryId,
    session.contentHash,
    session.nativeSource,
    session.title,
    session.author,
    session.initialPositionMs,
  );
}

export const toggleNativeAudiobook = () =>
  window.BokheimAndroid?.toggleAudiobookPlayback?.();
export const seekNativeAudiobookTo = (positionMs: number) =>
  window.BokheimAndroid?.seekAudiobookTo?.(positionMs);
export const seekNativeAudiobookBy = (offsetMs: number) =>
  window.BokheimAndroid?.seekAudiobookBy?.(offsetMs);
export const setNativeAudiobookPlaybackRate = (rate: number) =>
  window.BokheimAndroid?.setAudiobookPlaybackRate?.(rate);
export const closeNativeAudiobook = () => window.BokheimAndroid?.closeAudiobook?.();

export function isIgnoringBatteryOptimizations(): boolean | null {
  try {
    return window.BokheimAndroid?.isIgnoringBatteryOptimizations?.() ?? null;
  } catch {
    return null;
  }
}

export function openBatteryOptimizationSettings() {
  window.BokheimAndroid?.openBatteryOptimizationSettings?.();
}

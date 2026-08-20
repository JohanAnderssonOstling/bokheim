import { APP_NAME_LOWER } from "./application-name";

export type ApkUpdateState = {
  phase:
    | "idle"
    | "checking"
    | "available"
    | "current"
    | "downloading"
    | "permission_required"
    | "installing"
    | "error";
  version: string | null;
  currentVersion: string | null;
  url: string | null;
  sha256: string | null;
  fileName: string | null;
  progress: number | null;
  error: string | null;
};

const UPDATE_EVENT = `${APP_NAME_LOWER}-apk-update`;
export const EMPTY_APK_UPDATE_STATE: ApkUpdateState = {
  phase: "idle",
  version: null,
  currentVersion: null,
  url: null,
  sha256: null,
  fileName: null,
  progress: null,
  error: null,
};
let lastState: ApkUpdateState = EMPTY_APK_UPDATE_STATE;

function normalize(value: unknown): ApkUpdateState {
  if (!value || typeof value !== "object") return EMPTY_APK_UPDATE_STATE;
  const state = value as Partial<ApkUpdateState>;
  const phases = new Set<ApkUpdateState["phase"]>([
    "idle",
    "checking",
    "available",
    "current",
    "downloading",
    "permission_required",
    "installing",
    "error",
  ]);
  return {
    phase: phases.has(state.phase as ApkUpdateState["phase"])
      ? (state.phase as ApkUpdateState["phase"])
      : "error",
    version: typeof state.version === "string" ? state.version : null,
    currentVersion:
      typeof state.currentVersion === "string" ? state.currentVersion : null,
    url: typeof state.url === "string" ? state.url : null,
    sha256: typeof state.sha256 === "string" ? state.sha256 : null,
    fileName: typeof state.fileName === "string" ? state.fileName : null,
    progress:
      typeof state.progress === "number" && Number.isFinite(state.progress)
        ? Math.max(0, Math.min(1, state.progress))
        : null,
    error: typeof state.error === "string" ? state.error : null,
  };
}

export function apkUpdaterAvailable(): boolean {
  try {
    return window.BokheimAndroid?.isApkUpdaterAvailable?.() === true;
  } catch {
    return false;
  }
}

export function observeApkUpdates(handler: (state: ApkUpdateState) => void) {
  const listener = (event: Event) => {
    lastState = normalize((event as CustomEvent<unknown>).detail);
    handler(lastState);
  };
  window.addEventListener(UPDATE_EVENT, listener);
  handler(lastState);
  return () => window.removeEventListener(UPDATE_EVENT, listener);
}

export function checkForApkUpdate(force = false) {
  window.BokheimAndroid?.checkForApkUpdate?.(force);
}

export function installApkUpdate(state: ApkUpdateState) {
  if (!state.url || !state.sha256 || !state.fileName) return;
  window.BokheimAndroid?.downloadAndInstallApk?.(state.url, state.sha256, state.fileName);
}

export function openApkInstallSettings() {
  window.BokheimAndroid?.openApkInstallSettings?.();
}

import { isTauri } from "@tauri-apps/api/core";

export const isNativeApp = isTauri();
export const isWebApp = !isNativeApp;

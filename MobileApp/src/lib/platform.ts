import { isTauri } from "@tauri-apps/api/core";

export const isNativeApp = isTauri();
export const isWebApp = !isNativeApp && import.meta.env.VITE_BOKHEIM_BACKEND === "http";

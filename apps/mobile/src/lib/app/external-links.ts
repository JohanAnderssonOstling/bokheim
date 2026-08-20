import { openUrl } from "@tauri-apps/plugin-opener";
import { isNativeApp } from "./platform";

export async function openExternalUrl(url: string) {
  if (isNativeApp) {
    await openUrl(url);
    return;
  }
  window.open(url, "_blank", "noopener,noreferrer");
}

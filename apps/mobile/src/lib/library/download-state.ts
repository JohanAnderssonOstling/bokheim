import type { DownloadState } from "../shared/types";

export function isDownloaded(state: DownloadState | undefined): boolean {
  return state === "Downloaded";
}

export function isDownloadActive(state: DownloadState | undefined): boolean {
  return state === "Queued" || (typeof state === "object" && "Downloading" in state);
}

export function downloadProgress(state: DownloadState | undefined): number {
  if (state === "Downloaded") return 1;
  if (typeof state === "object" && "Downloading" in state) {
    return Math.min(1, Math.max(0, state.Downloading ?? 0));
  }
  return 0;
}

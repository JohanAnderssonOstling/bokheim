import { onMount } from "svelte";
import { listen } from "./app-events";
import { isWebApp } from "./platform";
import { APP_NAME_LOWER } from "./application-name";
import {
  nativeAudiobookAvailable,
  observeNativeAudiobookState,
  type NativeAudiobookState,
} from "../reader/native-audiobook";
import { APP_NAV_ITEMS, NAV_ITEMS, type Destination } from "./navigation";
import type { AppUiSession } from "../session/ui-session";
import type {
  AccountStatus,
  DownloadState,
  FolderDownloadStatus,
  NetworkStatus,
} from "../shared/types";
import type { NetworkTransport } from "../bindings";

const NETWORK_CHANGED_EVENT = `${APP_NAME_LOWER}-network-changed`;
const NETWORK_TRANSPORTS = new Set<NetworkTransport>([
  "offline",
  "wifi",
  "cellular",
  "ethernet",
  "vpn",
  "other",
  "unknown",
]);

export const UNKNOWN_NETWORK_STATUS: NetworkStatus = {
  transport: "unknown",
  connected: true,
  validated: null,
};

function parseNetworkStatus(value: unknown): NetworkStatus | null {
  if (!value || typeof value !== "object") return null;
  const candidate = value as Partial<NetworkStatus>;
  if (
    !NETWORK_TRANSPORTS.has(candidate.transport as NetworkTransport) ||
    typeof candidate.connected !== "boolean"
  )
    return null;
  if (candidate.validated !== null && typeof candidate.validated !== "boolean")
    return null;
  return candidate as NetworkStatus;
}

function readAndroidNetworkStatus(): NetworkStatus | null {
  const bridge = (
    window as Window & { BokheimAndroid?: { getNetworkStatus?: () => string } }
  ).BokheimAndroid;
  if (!bridge?.getNetworkStatus) return null;
  try {
    return parseNetworkStatus(JSON.parse(bridge.getNetworkStatus()));
  } catch {
    return null;
  }
}

function observeAndroidNetworkStatus(handler: (status: NetworkStatus) => void) {
  const listener = (event: Event) => {
    const status = parseNetworkStatus((event as CustomEvent<unknown>).detail);
    if (status) handler(status);
  };
  window.addEventListener(NETWORK_CHANGED_EVENT, listener);
  handler(readAndroidNetworkStatus() ?? UNKNOWN_NETWORK_STATUS);
  return () => window.removeEventListener(NETWORK_CHANGED_EVENT, listener);
}

export type DownloadStatusEvent = {
  library_id: string;
  event: { DownloadStatusChanged: { content_hash: string; state: DownloadState } };
};

export type FolderDownloadStatusEvent = {
  library_id: string;
  event: {
    FolderDownloadStatusChanged: {
      dir_id: string;
      downloaded_books: number;
      total_books: number;
    };
  };
};

type LifecycleOptions = {
  isWebApp: boolean;
  getActiveLibraryId: () => string | undefined;
  getActiveDestination: () => Destination;
  getReaderOpen: () => boolean;
  isSignedIn: () => boolean;
  /** The platform's startup sequence; see `startup-native` and `startup-web`. */
  startApplication: () => Promise<void>;
  loadServerTransferState: () => void | Promise<void>;
  synchronizeForegroundWebApp: (force?: boolean) => void | Promise<void>;
  focusContextualSearch: () => void;
  onNetworkStatus: (status: NetworkStatus) => void | Promise<void>;
  onNativeAudiobookState: (state: NativeAudiobookState) => void | Promise<void>;
  onDownloadStatus: (event: DownloadStatusEvent) => void;
  onFolderDownloadStatus: (event: FolderDownloadStatusEvent) => void;
  setAccountError: (message: string) => void;
  clearThumbnailCache: () => void;
  onDestinationShortcut?: (destination: Destination) => void;
};

function errorMessage(reason: unknown) {
  return reason instanceof Error ? reason.message : String(reason);
}

/** Registers application startup, native observers, global listeners, and cleanup. */
export function registerAppLifecycle(options: LifecycleOptions) {
  onMount(() => {
    let disposed = false;
    let unlistenDownloadStatus: (() => void) | undefined;
    let unlistenFolderDownloadStatus: (() => void) | undefined;
    let unlistenTransferStatus: (() => void) | undefined;
    let unlistenCatalogueChanged: (() => void) | undefined;
    let transferRefreshTimer: ReturnType<typeof setTimeout> | undefined;
    let webWasBackgrounded = false;

    const unobserveNativeAudiobook = nativeAudiobookAvailable()
      ? observeNativeAudiobookState((state) => void options.onNativeAudiobookState(state))
      : () => undefined;
    const unobserveNetwork = options.isWebApp
      ? () => undefined
      : observeAndroidNetworkStatus((status) => {
          void Promise.resolve(options.onNetworkStatus(status)).catch(() => undefined);
        });
    void options.startApplication();

    void listen<DownloadStatusEvent>("library:download-status", ({ payload }) => {
      if (payload.library_id === options.getActiveLibraryId())
        options.onDownloadStatus(payload);
    })
      .then((unlisten) => {
        if (disposed) unlisten();
        else unlistenDownloadStatus = unlisten;
      })
      .catch(() => undefined);

    void listen<FolderDownloadStatusEvent>(
      "library:folder-download-status",
      ({ payload }) => {
        if (payload.library_id === options.getActiveLibraryId())
          options.onFolderDownloadStatus(payload);
      },
    )
      .then((unlisten) => {
        if (disposed) unlisten();
        else unlistenFolderDownloadStatus = unlisten;
      })
      .catch(() => undefined);

    void listen("library:transfer-status", () => {
      if (!options.isSignedIn() || options.getActiveDestination() !== "account") return;
      if (transferRefreshTimer) clearTimeout(transferRefreshTimer);
      transferRefreshTimer = setTimeout(() => {
        void Promise.resolve(options.loadServerTransferState()).catch((reason) =>
          options.setAccountError(errorMessage(reason)),
        );
      }, 120);
    })
      .then((unlisten) => {
        if (disposed) unlisten();
        else unlistenTransferStatus = unlisten;
      })
      .catch(() => undefined);

    void listen("library:catalogue-changed", () => {
      if (options.isSignedIn())
        void Promise.resolve(options.loadServerTransferState()).catch((reason) =>
          options.setAccountError(errorMessage(reason)),
        );
    })
      .then((unlisten) => {
        if (disposed) unlisten();
        else unlistenCatalogueChanged = unlisten;
      })
      .catch(() => undefined);

    const handleShortcut = (event: KeyboardEvent) => {
      if (
        options.isWebApp ||
        options.getReaderOpen() ||
        !(event.ctrlKey || event.metaKey) ||
        event.altKey ||
        event.shiftKey
      )
        return;
      const destination = [...NAV_ITEMS, ...APP_NAV_ITEMS][Number(event.key) - 1]?.id;
      if (destination) {
        event.preventDefault();
        options.onDestinationShortcut?.(destination);
      } else if (event.key.toLocaleLowerCase() === "f") {
        event.preventDefault();
        options.focusContextualSearch();
      }
    };
    const handleVisibilityChange = () => {
      if (document.visibilityState === "hidden") webWasBackgrounded = true;
      else if (webWasBackgrounded) {
        webWasBackgrounded = false;
        void options.synchronizeForegroundWebApp(true);
      }
    };
    const handleWindowFocus = () => void options.synchronizeForegroundWebApp();

    document.addEventListener("keydown", handleShortcut);
    if (options.isWebApp) {
      document.addEventListener("visibilitychange", handleVisibilityChange);
      window.addEventListener("focus", handleWindowFocus);
    }

    return () => {
      disposed = true;
      unlistenDownloadStatus?.();
      unlistenFolderDownloadStatus?.();
      unlistenTransferStatus?.();
      unlistenCatalogueChanged?.();
      if (transferRefreshTimer) clearTimeout(transferRefreshTimer);
      document.removeEventListener("keydown", handleShortcut);
      document.removeEventListener("visibilitychange", handleVisibilityChange);
      window.removeEventListener("focus", handleWindowFocus);
      unobserveNetwork();
      unobserveNativeAudiobook();
      options.clearThumbnailCache();
    };
  });
}

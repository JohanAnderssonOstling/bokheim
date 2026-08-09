import type { NetworkStatus, NetworkTransport } from "./bindings";
import { APP_NAME_LOWER } from "./application-name";
import { SETTINGS_CONTRACT } from "./types";

const EVENT_NAME = `${APP_NAME_LOWER}-network-changed`;
const TRANSPORTS = new Set<NetworkTransport>(["offline", "wifi", "cellular", "ethernet", "vpn", "other", "unknown"]);

export const UNKNOWN_NETWORK_STATUS: NetworkStatus = {
  transport: "unknown",
  connected: true,
  metered: null,
  validated: null,
};

function parseNetworkStatus(value: unknown): NetworkStatus | null {
  if (!value || typeof value !== "object") return null;
  const candidate = value as Partial<NetworkStatus>;
  if (!TRANSPORTS.has(candidate.transport as NetworkTransport) || typeof candidate.connected !== "boolean") return null;
  if (candidate.metered !== null && typeof candidate.metered !== "boolean") return null;
  if (candidate.validated !== null && typeof candidate.validated !== "boolean") return null;
  return candidate as NetworkStatus;
}

function readAndroidStatus(): NetworkStatus | null {
  const bridge = (window as Window & { BokheimAndroid?: { getNetworkStatus?: () => string } }).BokheimAndroid;
  if (!bridge?.getNetworkStatus) return null;
  try {
    return parseNetworkStatus(JSON.parse(bridge.getNetworkStatus()));
  } catch {
    return null;
  }
}

export function observeNetworkStatus(handler: (status: NetworkStatus) => void) {
  const listener = (event: Event) => {
    const status = parseNetworkStatus((event as CustomEvent<unknown>).detail);
    if (status) handler(status);
  };
  window.addEventListener(EVENT_NAME, listener);
  handler(readAndroidStatus() ?? UNKNOWN_NETWORK_STATUS);
  return () => window.removeEventListener(EVENT_NAME, listener);
}

export function networkStatusLabel(status: NetworkStatus) {
  if (!status.connected || status.transport === "offline") return "Offline";
  return SETTINGS_CONTRACT.network_transports.find((definition) => definition.transport === status.transport)?.label ?? "Network unknown";
}

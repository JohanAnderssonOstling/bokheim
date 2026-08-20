import type {
  AccountStorageUsage,
  LibrarySyncStatus,
  ServerLibraryStorageUsage,
} from "./types";

/**
 * Mirrors `LibrarySyncSummary` in `client/app-contract/src/account.rs`.
 *
 * The account page opens with a verdict, and desktop and mobile must not
 * disagree about it, so the rule lives in Rust and is mirrored here with the
 * same fixtures in `sync-summary.test.ts`.
 */
export type LibrarySyncSummary = {
  libraryCount: number;
  attentionCount: number;
  lastSyncedAt: number | null;
};

/**
 * A library reporting `synced` that has never completed a synchronization, or
 * that holds local edits, is not settled either — the page must not claim it is.
 */
export function needsAttention(status: LibrarySyncStatus) {
  return (
    status.state !== "synced" ||
    status.has_unsynced_changes ||
    status.last_synced_at === null
  );
}

export function summarizeSync(statuses: LibrarySyncStatus[]): LibrarySyncSummary {
  const synced = statuses
    .map((status) => status.last_synced_at)
    .filter((value): value is number => value !== null);
  return {
    libraryCount: statuses.length,
    attentionCount: statuses.filter(needsAttention).length,
    lastSyncedAt: synced.length ? Math.max(...synced) : null,
  };
}

/** Deliberately false with no libraries: an empty device has nothing to sync. */
export function allSynced(summary: LibrarySyncSummary) {
  return summary.libraryCount > 0 && summary.attentionCount === 0;
}

/** The backend reports byte counts as optional numbers; absent means zero. */
export function bytesOf(bytes: number | null) {
  return bytes ?? 0;
}

export function formatBytes(bytes: number | null) {
  const value = bytesOf(bytes);
  if (value < 1_000) return `${value} B`;
  const units = ["KB", "MB", "GB", "TB", "PB"];
  let scaled = value;
  let unit = -1;
  do {
    scaled /= 1_000;
    unit += 1;
  } while (scaled >= 1_000 && unit < units.length - 1);
  return `${new Intl.NumberFormat(undefined, { maximumFractionDigits: scaled < 10 ? 2 : 1 }).format(scaled)} ${units[unit]}`;
}

export function formatLastSync(timestamp: number | null) {
  if (timestamp === null) return "never synced";
  const elapsedSeconds = Math.max(0, Math.round((Date.now() - timestamp) / 1_000));
  if (elapsedSeconds < 60) return "last sync now";
  if (elapsedSeconds < 60 * 60) return `last sync ${Math.floor(elapsedSeconds / 60)}m`;
  if (elapsedSeconds < 24 * 60 * 60)
    return `last sync ${Math.floor(elapsedSeconds / (60 * 60))}h`;
  const days = Math.floor(elapsedSeconds / (24 * 60 * 60));
  return `last sync ${days === 1 ? "1 day" : `${days} days`}`;
}

export type StorageSegment = {
  label: string;
  bytes: number;
  /** Share of the whole quota, as a percentage for CSS. */
  percent: number;
  tone: "library" | "unattributed" | "reserved";
  /** Index into the accent shade ramp, for `tone: "library"`. */
  shade: number;
};

const LEGEND_LIBRARY_LIMIT = 4;

/**
 * Splits the quota into the parts that explain it, entirely from what the
 * server reports: what each library is charged, what is charged to no library
 * this device knows about, and what uploads have reserved.
 *
 * Mirrors `SyncPage::storage_segments` in the desktop frontend. The server
 * attributes each charged blob to exactly one library, so these shares
 * partition `used_bytes` and can be drawn against the quota without clamping.
 * Nothing here comes from local disk: what is downloaded to this device is a
 * different quantity and is reported beside the libraries, never in this meter.
 */
export function storageSegments(
  usage: AccountStorageUsage,
  server: ServerLibraryStorageUsage[],
  names: Map<string, string>,
  copy: { unattributed: string; reserved: string; otherLibraries: string },
): StorageSegment[] {
  const quota = bytesOf(usage.quota_bytes);
  if (quota <= 0) return [];
  const share = (bytes: number) => (bytes / quota) * 100;
  const ordered = [...server]
    .filter((entry) => bytesOf(entry.used_bytes) > 0)
    .sort(
      (left, right) =>
        bytesOf(right.used_bytes) - bytesOf(left.used_bytes) ||
        left.library_id.localeCompare(right.library_id),
    );

  const segments: StorageSegment[] = [];
  let attributed = 0;
  let otherBytes = 0;
  ordered.forEach((entry, index) => {
    const bytes = bytesOf(entry.used_bytes);
    attributed += bytes;
    if (index < LEGEND_LIBRARY_LIMIT)
      segments.push({
        label: names.get(entry.library_id) ?? entry.library_id,
        bytes,
        percent: share(bytes),
        tone: "library",
        shade: index,
      });
    else otherBytes += bytes;
  });
  if (otherBytes > 0)
    segments.push({
      label: copy.otherLibraries,
      bytes: otherBytes,
      percent: share(otherBytes),
      tone: "library",
      shade: LEGEND_LIBRARY_LIMIT,
    });
  // Charged bytes belonging to no library this device knows about. Never
  // negative, whatever the two figures disagree about.
  const unattributed = Math.max(0, bytesOf(usage.used_bytes) - attributed);
  if (unattributed > 0)
    segments.push({
      label: copy.unattributed,
      bytes: unattributed,
      percent: share(unattributed),
      tone: "unattributed",
      shade: 0,
    });
  const reserved = bytesOf(usage.reserved_bytes);
  if (reserved > 0)
    segments.push({
      label: copy.reserved,
      bytes: reserved,
      percent: share(reserved),
      tone: "reserved",
      shade: 0,
    });
  return segments;
}

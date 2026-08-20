<script lang="ts">
  import PrimaryButton from "../shared/buttons/PrimaryButton.svelte";
  import SecondaryButton from "../shared/buttons/SecondaryButton.svelte";
  import { ACCOUNT_CONTRACT } from "../../lib/shared/types";
  import {
    allSynced,
    bytesOf,
    formatBytes,
    formatLastSync,
    storageSegments,
    summarizeSync,
  } from "../../lib/shared/sync-summary";
  import type {
    AccountStatus,
    AccountStorageUsage,
    LibrarySyncStatus,
    LibraryTransfers,
    LocalLibraryStorageUsage,
    ServerLibraryStorageUsage,
  } from "../../lib/shared/types";

  let {
    account,
    accountBusy,
    accountProgress,
    accountError,
    storageUsage,
    librarySyncStatuses,
    libraryTransfers,
    localLibraryStorageUsage,
    serverLibraryStorageUsage,
    onrefresh,
    onlogout,
  }: {
    account: AccountStatus;
    accountBusy: boolean;
    accountProgress: string;
    accountError: string;
    storageUsage: AccountStorageUsage | null;
    librarySyncStatuses: LibrarySyncStatus[];
    libraryTransfers: LibraryTransfers[];
    localLibraryStorageUsage: LocalLibraryStorageUsage[];
    /** `null` from a server without the per-library storage endpoint. */
    serverLibraryStorageUsage: ServerLibraryStorageUsage[] | null;
    onrefresh: () => Promise<boolean>;
    onlogout: () => void | Promise<void>;
  } = $props();

  const SUMMARY = ACCOUNT_CONTRACT.summary;

  let activityExpanded = $state(false);

  const summary = $derived(summarizeSync(librarySyncStatuses));
  const settled = $derived(allSynced(summary));

  const breakdownSupported = $derived(serverLibraryStorageUsage !== null);

  /** Libraries sorted by the quantity the page leads with. */
  const librariesBySize = $derived(
    [...librarySyncStatuses].sort(
      (left, right) =>
        sortBytes(right.library_id) - sortBytes(left.library_id) ||
        left.library_name.localeCompare(right.library_name),
    ),
  );
  const largestLibraryBytes = $derived(
    Math.max(0, ...librarySyncStatuses.map((library) => sortBytes(library.library_id))),
  );
  const libraryNames = $derived(
    new Map(
      librarySyncStatuses.map((library) => [library.library_id, library.library_name]),
    ),
  );
  const segments = $derived(
    storageUsage && serverLibraryStorageUsage
      ? storageSegments(storageUsage, serverLibraryStorageUsage, libraryNames, {
          unattributed: SUMMARY.unattributed_label,
          reserved: SUMMARY.reserved_label,
          otherLibraries: SUMMARY.other_libraries_label,
        })
      : [],
  );

  const headline = $derived(
    accountBusy
      ? SUMMARY.syncing_label
      : settled
        ? SUMMARY.all_synced_label
        : summary.libraryCount === 0
          ? ACCOUNT_CONTRACT.no_local_libraries_label
          : SUMMARY.attention_label,
  );

  /** How many libraries, how recently they synced, and whose account this is. */
  const verdictDetail = $derived(
    [
      summary.attentionCount === 0
        ? libraryCountLabel()
        : `${summary.attentionCount} of ${libraryCountLabel()} ${SUMMARY.needs_attention_suffix}`,
      formatLastSync(summary.lastSyncedAt),
      account.email ?? "",
    ]
      .filter(Boolean)
      .join(" · "),
  );

  function libraryCountLabel() {
    return summary.libraryCount === 1 ? "1 library" : `${summary.libraryCount} libraries`;
  }

  function localBytes(libraryId: string) {
    return bytesOf(
      localLibraryStorageUsage.find((usage) => usage.library_id === libraryId)
        ?.used_bytes ?? 0,
    );
  }

  /** What the account is charged for this library, or null if unreported. */
  function serverBytes(libraryId: string) {
    if (serverLibraryStorageUsage === null) return null;
    return bytesOf(
      serverLibraryStorageUsage.find((usage) => usage.library_id === libraryId)
        ?.used_bytes ?? 0,
    );
  }

  function sortBytes(libraryId: string) {
    return serverBytes(libraryId) ?? localBytes(libraryId);
  }

  function activeUploads(libraryId: string) {
    return (
      libraryTransfers
        .find((group) => group.library_id === libraryId)
        ?.transfers.filter(
          (transfer) =>
            transfer.kind === "upload_book" || transfer.kind === "upload_thumbnail",
        ) ?? []
    );
  }

  function syncStatusText(library: LibrarySyncStatus) {
    const uploads = activeUploads(library.library_id);
    if (uploads.some((transfer) => transfer.state === "retrying"))
      return "Waiting to retry";
    if (uploads.length)
      return `${uploads.length} ${uploads.length === 1 ? "file" : "files"} remaining`;
    const sync = formatLastSync(library.last_synced_at);
    return sync.charAt(0).toUpperCase() + sync.slice(1);
  }

  function syncStateLabel(library: LibrarySyncStatus) {
    if (library.state === "synced" && library.last_synced_at === null)
      return "Not synced";
    if (library.state === "synced" && library.has_unsynced_changes)
      return "Changes pending";
    return (
      ACCOUNT_CONTRACT.sync_states.find(
        (definition) => definition.state === library.state,
      )?.label ?? library.state
    );
  }

  function transferKindLabel(kind: string) {
    const [direction, asset] = kind.split("_");
    return `${direction === "upload" ? "Uploading" : "Downloading"} ${asset === "book" ? "books" : "thumbnails"}`;
  }

  function transferHistoryLabel(kind: string) {
    const [direction, asset] = kind.split("_");
    return `${direction === "upload" ? "Uploaded" : "Downloaded"} ${asset === "book" ? "books" : "thumbnails"}`;
  }

  function activeTransferCount() {
    return libraryTransfers.reduce((count, group) => count + group.transfers.length, 0);
  }

  function transferHistoryCount() {
    return libraryTransfers.reduce((count, group) => count + group.history.length, 0);
  }

  /** Mirrors `AccountStorageUsage::available_bytes`, which saturates at zero. */
  function availableBytes(usage: AccountStorageUsage) {
    return Math.max(
      0,
      bytesOf(usage.quota_bytes) -
        bytesOf(usage.used_bytes) -
        bytesOf(usage.reserved_bytes),
    );
  }

  function barPercent(libraryId: string) {
    return largestLibraryBytes === 0
      ? 0
      : (sortBytes(libraryId) / largestLibraryBytes) * 100;
  }
</script>

<section
  class="account-page"
  aria-label={ACCOUNT_CONTRACT.title}
>
  <div class="account-panel">
    <div class="account-content">
      <div
        class="verdict"
        class:attention={!settled}
      >
        <div class="verdict-copy">
          <strong>{headline}</strong>
          <small>{verdictDetail}</small>
        </div>
        <div class="account-actions">
          <PrimaryButton
            disabled={accountBusy}
            onclick={() => void onrefresh()}>{SUMMARY.sync_now_label}</PrimaryButton
          >
          <SecondaryButton
            disabled={accountBusy}
            onclick={() => void onlogout()}
            >{ACCOUNT_CONTRACT.sign_out_label}</SecondaryButton
          >
        </div>
      </div>

      {#if storageUsage}
        <div
          class="storage-usage"
          aria-label={SUMMARY.storage_section_label}
        >
          <div class="subheading"><span>{SUMMARY.storage_section_label}</span></div>
          <div class="storage-figures">
            <span
              ><strong>{formatBytes(storageUsage.used_bytes)}</strong> of {formatBytes(
                storageUsage.quota_bytes,
              )} used</span
            >
            <small>{formatBytes(availableBytes(storageUsage))} available</small>
          </div>
          {#if segments.length}
            <div
              class="meter"
              role="img"
              aria-label="{formatBytes(storageUsage.used_bytes)} of {formatBytes(
                storageUsage.quota_bytes,
              )} used"
            >
              {#each segments as segment, index (segment.label + index)}
                <span
                  class="segment {segment.tone}"
                  data-shade={segment.shade}
                  style="width: {segment.percent}%"
                ></span>
              {/each}
            </div>
            <ul class="legend">
              {#each segments as segment, index (segment.label + index)}
                <li>
                  <i
                    class="segment {segment.tone}"
                    data-shade={segment.shade}
                  ></i>{segment.label}
                  {formatBytes(segment.bytes)}
                </li>
              {/each}
            </ul>
          {/if}
        </div>
      {/if}

      <div
        class="server-libraries"
        aria-label={ACCOUNT_CONTRACT.libraries_section_label}
      >
        <div class="subheading">
          <span>{ACCOUNT_CONTRACT.libraries_section_label}</span>
        </div>
        {#if !breakdownSupported}<small>{SUMMARY.breakdown_unavailable_label}</small>{/if}
        {#each librariesBySize as library (library.library_id)}
          <div class="library-row">
            <span class="library-copy"
              ><strong>{library.library_name}</strong><small
                >{syncStatusText(library)}</small
              ></span
            >
            <span
              class="sync-membership"
              class:attention={library.state !== "synced" ||
                library.has_unsynced_changes ||
                library.last_synced_at === null}>{syncStateLabel(library)}</span
            >
            <span class="usage">
              {#if breakdownSupported}<small class="primary"
                  >{SUMMARY.server_usage_label}
                  {formatBytes(serverBytes(library.library_id))}</small
                >{/if}
              <small
                >{SUMMARY.device_usage_label}
                {formatBytes(localBytes(library.library_id))}</small
              >
              <span class="bar"
                ><span style="width: {barPercent(library.library_id)}%"></span></span
              >
            </span>
          </div>
        {:else}
          <p class="no-transfers">{ACCOUNT_CONTRACT.no_local_libraries_label}</p>
        {/each}
      </div>

      {#if activeTransferCount() > 0}
        <div
          class="server-libraries"
          aria-label={ACCOUNT_CONTRACT.transfers_section_label}
        >
          <div class="subheading">
            <span>{ACCOUNT_CONTRACT.transfers_section_label}</span><small
              >{activeTransferCount()}</small
            >
          </div>
          {#each libraryTransfers as group (group.library_id)}
            {#each group.transfers as transfer (transfer.id)}
              <div class="transfer-row">
                <span class="library-copy"
                  ><strong>{transferKindLabel(transfer.kind)}</strong><small
                    >{transfer.completed_items}/{transfer.total_items} · {group.library_name}{#if transfer.attempts}
                      · attempt {transfer.attempts}{/if}{#if transfer.last_error}
                      · {transfer.last_error}{/if}</small
                  ></span
                >
                <span
                  class:running={transfer.state === "running"}
                  class="sync-membership">{transfer.state}</span
                >
              </div>
            {/each}
          {/each}
        </div>
      {/if}

      <div class="activity-footer">
        <small
          >{transferHistoryCount() === 0
            ? SUMMARY.activity_idle_label
            : `${transferHistoryCount()} ${SUMMARY.finished_suffix}`}</small
        >
        <SecondaryButton
          disabled={transferHistoryCount() === 0}
          onclick={() => (activityExpanded = !activityExpanded)}
        >
          {activityExpanded ? SUMMARY.hide_activity_label : SUMMARY.show_activity_label}
        </SecondaryButton>
      </div>

      {#if activityExpanded && transferHistoryCount() > 0}
        <div
          class="server-libraries"
          aria-label={ACCOUNT_CONTRACT.transfer_history_section_label}
        >
          {#each libraryTransfers as group (group.library_id)}
            {#each group.history as transfer (transfer.id)}
              <div class="transfer-row">
                <span class="library-copy"
                  ><strong>{transferHistoryLabel(transfer.kind)}</strong><small
                    >{transfer.completed_items}/{transfer.total_items} · {group.library_name}{#if transfer.last_error}
                      · {transfer.last_error}{/if}</small
                  ></span
                >
                <span class="sync-membership">{transfer.state}</span>
              </div>
            {/each}
          {/each}
        </div>
      {/if}

      {#if accountProgress}<p
          class="account-progress"
          aria-live="polite"
        >
          {accountProgress}
        </p>{/if}
      {#if accountError}<p
          class="account-error"
          role="alert"
        >
          {accountError}
        </p>{/if}
    </div>
  </div>
</section>

<style>
  .account-page {
    width: 100%;
    min-height: 100%;
    padding: var(--layout-content-padding);
  }
  .account-panel {
    width: min(100%, 57.5rem);
    margin: 0 auto;
    padding: 1.125rem;
    border: 1px solid var(--color-border);
    border-radius: 0;
    color: var(--color-ink-text);
    background: var(--color-raised-bg);
  }
  .account-content {
    display: grid;
    gap: 1rem;
  }
  .account-actions {
    display: flex;
    flex-wrap: wrap;
    gap: 0.5rem;
  }

  /* The page opens with its conclusion; the stripe carries the state so the
     headline itself does not have to be coloured. */
  .verdict {
    display: grid;
    gap: 0.8rem;
    padding: 0.75rem 0.75rem 0.75rem 1rem;
    border: 1px solid var(--color-border);
    border-left: 3px solid var(--color-accent);
    border-radius: 0;
  }
  .verdict.attention {
    border-left-color: var(--color-warning-text);
  }
  .verdict-copy {
    display: grid;
    gap: 0.2rem;
    min-width: 0;
  }
  .verdict-copy strong {
    font-size: 1.05rem;
  }
  .verdict-copy small {
    overflow-wrap: anywhere;
  }

  .storage-usage {
    display: grid;
    gap: 0.45rem;
  }
  .storage-figures {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 0.75rem;
    font-size: 0.78rem;
    color: var(--color-subtle-text);
  }
  .storage-figures strong {
    font-size: 1.35rem;
    color: var(--color-ink-text);
  }
  .meter {
    display: flex;
    overflow: hidden;
    width: 100%;
    height: 0.55rem;
    border: 1px solid var(--color-border);
    border-radius: 0;
    background: color-mix(in srgb, var(--color-border) 30%, transparent);
  }
  .segment {
    display: block;
    height: 100%;
  }
  /* Shades walk away from the accent rather than introducing a second hue. */
  .segment.library[data-shade="0"] {
    background: var(--color-accent);
  }
  .segment.library[data-shade="1"] {
    background: color-mix(in srgb, var(--color-accent) 78%, var(--color-raised-bg));
  }
  .segment.library[data-shade="2"] {
    background: color-mix(in srgb, var(--color-accent) 58%, var(--color-raised-bg));
  }
  .segment.library[data-shade="3"] {
    background: color-mix(in srgb, var(--color-accent) 40%, var(--color-raised-bg));
  }
  .segment.library[data-shade="4"] {
    background: color-mix(in srgb, var(--color-accent) 26%, var(--color-raised-bg));
  }
  .segment.unattributed {
    background: repeating-linear-gradient(
      135deg,
      var(--color-border) 0 2px,
      transparent 2px 5px
    );
  }
  .segment.reserved {
    background: var(--color-warning-text);
  }
  .legend {
    display: flex;
    flex-wrap: wrap;
    gap: 0.2rem 1rem;
    margin: 0;
    padding: 0;
    list-style: none;
    color: var(--color-subtle-text);
    font-size: 0.7rem;
  }
  .legend li {
    display: flex;
    align-items: center;
    gap: 0.35rem;
  }
  .legend i {
    width: 0.5rem;
    height: 0.5rem;
    border-radius: 0;
  }

  .account-progress,
  .account-error {
    margin: 0;
    padding: 0.2rem 0.1rem;
    font-size: 0.72rem;
  }
  .account-progress {
    color: var(--color-subtle-text);
  }
  .account-error {
    color: var(--color-danger-text);
    overflow-wrap: anywhere;
  }
  .server-libraries {
    display: grid;
    gap: 0.45rem;
  }
  .subheading {
    display: flex;
    align-items: center;
    justify-content: space-between;
    color: var(--color-subtle-text);
    font-size: 0.72rem;
    font-weight: 750;
  }
  small {
    color: var(--color-subtle-text);
    font-size: 0.7rem;
  }
  strong {
    font-size: 0.84rem;
  }
  .library-copy {
    display: grid;
    gap: 0.15rem;
    min-width: 0;
  }
  .library-copy strong,
  .library-copy small {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .library-row {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    align-items: center;
    gap: 0.35rem 0.75rem;
    min-height: 3.45rem;
    padding: 0.55rem 0.7rem;
    border-bottom: 1px solid var(--color-border);
  }
  .library-row:first-of-type {
    border-top: 1px solid var(--color-border);
  }
  /* The size and its comparison bar sit under the row so a narrow screen never
     squeezes the library name to fit them. */
  .usage {
    grid-column: 1 / -1;
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.35rem 0.75rem;
  }
  .usage .primary {
    color: var(--color-ink-text);
  }
  .usage .bar {
    flex: 1;
    height: 0.25rem;
    border-radius: 0;
    background: color-mix(in srgb, var(--color-border) 45%, transparent);
  }
  .usage .bar > span {
    display: block;
    height: 100%;
    border-radius: 0;
    background: var(--color-accent);
  }
  .sync-membership {
    padding: 0.25rem 0.48rem;
    border-radius: 0;
    color: var(--color-accent);
    background: color-mix(in srgb, var(--color-accent) 10%, transparent);
    font-size: 0.66rem;
    font-weight: 750;
  }
  .sync-membership.attention {
    color: var(--color-warning-text);
    background: var(--color-warning-bg);
  }
  .sync-membership.running {
    color: var(--color-raised-bg);
    background: var(--color-accent);
  }
  .transfer-row {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    align-items: center;
    gap: 0.75rem;
    min-height: 3.45rem;
    padding: 0.55rem 0.7rem;
    border-bottom: 1px solid var(--color-border);
  }
  .transfer-row small {
    overflow-wrap: anywhere;
    white-space: normal;
  }
  .no-transfers {
    margin: 0;
    padding: 0.7rem 0;
    color: var(--color-subtle-text);
    font-size: 0.72rem;
  }

  /* One line stands in for the batch job sections while they are empty. */
  .activity-footer {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: 0.5rem;
    padding: 0.55rem 0.7rem;
    border: 1px solid var(--color-border);
    border-radius: 0;
  }
</style>

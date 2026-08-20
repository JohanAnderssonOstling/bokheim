<script lang="ts">
  import StatusState from "../../components/shared/StatusState.svelte";
  import ServerAccountDialog from "../../components/shell/ServerAccountDialog.svelte";
  import type {
    AccountStatus,
    AccountStorageUsage,
    LibrarySyncStatus,
    LibraryTransfers,
    LocalLibraryStorageUsage,
    ServerLibraryStorageUsage,
  } from "../../lib/shared/types";

  let {
    resolved,
    statusError,
    account,
    busy,
    progress,
    error,
    storageUsage,
    librarySyncStatuses,
    libraryTransfers,
    localLibraryStorageUsage,
    serverLibraryStorageUsage,
    onrefresh,
    onretry,
    onlogout,
  }: {
    resolved: boolean;
    statusError: string;
    account: AccountStatus;
    busy: boolean;
    progress: string;
    error: string;
    storageUsage: AccountStorageUsage | null;
    librarySyncStatuses: LibrarySyncStatus[];
    libraryTransfers: LibraryTransfers[];
    localLibraryStorageUsage: LocalLibraryStorageUsage[];
    serverLibraryStorageUsage: ServerLibraryStorageUsage[] | null;
    onrefresh: () => Promise<boolean>;
    onretry: () => void | Promise<void>;
    onlogout: () => void | Promise<void>;
  } = $props();
</script>

{#if !resolved}
  <StatusState
    kind="loading"
    title="Checking your session"
  />
{:else if statusError}
  <StatusState
    kind="error"
    title="Account status unavailable"
    message={statusError}
    actionLabel="Retry"
    onaction={onretry}
  />
{:else if !account.signed_in}
  <StatusState
    kind="loading"
    title="Redirecting to sign in"
  />
{:else}
  <ServerAccountDialog
    {account}
    accountBusy={busy}
    accountProgress={progress}
    accountError={error}
    {storageUsage}
    {librarySyncStatuses}
    {libraryTransfers}
    {localLibraryStorageUsage}
    {serverLibraryStorageUsage}
    {onrefresh}
    {onlogout}
  />
{/if}

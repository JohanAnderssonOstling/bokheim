<script lang="ts">
  import { onMount } from "svelte";
  import { goto } from "$app/navigation";
  import AccountPage from "./AccountPage.svelte";
  import { commands } from "../../lib/app/commands";
  import { onLibraryRefresh } from "../../lib/app/library-events";
  import type {
    AccountStatus,
    AccountStorageUsage,
    LibrarySyncStatus,
    LibraryTransfers,
    LocalLibraryStorageUsage,
    ServerLibraryStorageUsage,
  } from "../../lib/shared/types";

  const EMPTY_ACCOUNT: AccountStatus = {
    signed_in: false,
    server_url: null,
    email: null,
  };

  let resolved = $state(false);
  let statusError = $state("");
  let account = $state<AccountStatus>(EMPTY_ACCOUNT);
  let busy = $state(false);
  let progress = $state("");
  let error = $state("");
  let storageUsage = $state<AccountStorageUsage | null>(null);
  let librarySyncStatuses = $state<LibrarySyncStatus[]>([]);
  let libraryTransfers = $state<LibraryTransfers[]>([]);
  let localLibraryStorageUsage = $state<LocalLibraryStorageUsage[]>([]);
  let serverLibraryStorageUsage = $state<ServerLibraryStorageUsage[] | null>(null);

  function message(reason: unknown) {
    return reason instanceof Error ? reason.message : String(reason);
  }

  async function load() {
    resolved = false;
    statusError = "";
    try {
      account = await commands.getAccountStatus();
      if (account.signed_in) await loadTransferState();
      else await goto("/account/login", { replaceState: true });
    } catch (reason) {
      statusError = `Could not check your session: ${message(reason)}`;
    } finally {
      resolved = true;
    }
  }

  async function loadTransferState() {
    const [libraryList, transfers, storage, localStorage, serverStorage] =
      await Promise.all([
        commands.reconcileLibraries(),
        commands.getTransferStatuses(),
        commands.getAccountStorageUsage().catch(() => null),
        commands.getLocalLibraryStorageUsage(),
        commands.getServerLibraryStorageUsage().catch(() => null),
      ]);
    librarySyncStatuses = libraryList.sync_statuses;
    libraryTransfers = transfers;
    storageUsage = storage;
    localLibraryStorageUsage = localStorage;
    serverLibraryStorageUsage = serverStorage;
  }

  async function run(progressLabel: string, request: () => Promise<unknown>) {
    if (busy) return false;
    busy = true;
    error = "";
    progress = progressLabel;
    try {
      await request();
      progress = "";
      return true;
    } catch (reason) {
      error = message(reason);
      progress = "";
      return false;
    } finally {
      busy = false;
    }
  }

  async function logout() {
    await run("Signing out…", async () => {
      account = await commands.logout();
      librarySyncStatuses = [];
      libraryTransfers = [];
      localLibraryStorageUsage = [];
      storageUsage = null;
      serverLibraryStorageUsage = null;
      await goto("/library", { replaceState: true });
    });
  }

  onMount(load);
  $effect(() => onLibraryRefresh(() => void loadTransferState()));
</script>

<svelte:head>
  <title>Account · Bokheim</title>
</svelte:head>

<AccountPage
  {resolved}
  {statusError}
  {account}
  {busy}
  {progress}
  {error}
  {storageUsage}
  {librarySyncStatuses}
  {libraryTransfers}
  {localLibraryStorageUsage}
  {serverLibraryStorageUsage}
  onrefresh={async () => {
    await load();
    return !statusError;
  }}
  onretry={load}
  onlogout={logout}
/>

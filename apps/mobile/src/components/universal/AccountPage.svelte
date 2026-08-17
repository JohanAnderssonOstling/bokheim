<script lang="ts">
  import ServerAccountDialog from "../shell/ServerAccountDialog.svelte";
  import type { AccountStatus, AccountStorageUsage, LibrarySyncStatus, LibraryTransfers, LocalLibraryStorageUsage, ServerLibraryStorageUsage } from "../../lib/types";

  let {
    resolved, statusError, account, busy, progress, error, storageUsage,
    librarySyncStatuses, libraryTransfers, localLibraryStorageUsage, serverLibraryStorageUsage,
    onrefresh, onretry, onlogin, onlogout, onregister, onresendverification,
    onverifyemail, onrequestpasswordreset, onresetpassword,
  }: {
    resolved: boolean; statusError: string; account: AccountStatus; busy: boolean; progress: string; error: string;
    storageUsage: AccountStorageUsage | null; librarySyncStatuses: LibrarySyncStatus[]; libraryTransfers: LibraryTransfers[];
    localLibraryStorageUsage: LocalLibraryStorageUsage[]; serverLibraryStorageUsage: ServerLibraryStorageUsage[] | null;
    onrefresh: () => Promise<boolean>; onretry: () => void | Promise<void>;
    onlogin: (email: string, password: string) => Promise<boolean>; onlogout: () => void | Promise<void>; onregister: (email: string, password: string) => Promise<boolean>;
    onresendverification: (email: string) => Promise<boolean>; onverifyemail: (email: string, pin: string) => Promise<boolean>;
    onrequestpasswordreset: (email: string) => Promise<boolean>; onresetpassword: (token: string, password: string) => Promise<boolean>;
  } = $props();
</script>

{#if !resolved}
  <section class="state" aria-live="polite"><span class="loader"></span><strong>Checking your session</strong></section>
{:else if statusError}
  <section class="state" role="alert"><strong>Account status unavailable</strong><p>{statusError}</p><button type="button" onclick={onretry}>Retry</button></section>
{:else}
  <ServerAccountDialog account={account} accountBusy={busy} accountProgress={progress} accountError={error} {storageUsage} {librarySyncStatuses} {libraryTransfers} {localLibraryStorageUsage} {serverLibraryStorageUsage} {onrefresh} {onlogin} {onlogout} {onregister} {onresendverification} {onverifyemail} {onrequestpasswordreset} {onresetpassword} />
{/if}

<style>
  .state { display: grid; width: min(100%, 36rem); margin: 2rem auto; padding: 1.25rem; justify-items: center; gap: .7rem; color: var(--color-subtle-text); text-align: center; }
  .state button { min-height: 2.5rem; padding: .5rem .9rem; border: 1px solid var(--color-accent); border-radius: 0; color: var(--color-raised-bg); background: var(--color-accent); font: inherit; font-weight: 700; }
  .loader { width: 1.4rem; height: 1.4rem; border: .18rem solid var(--color-border); border-top-color: var(--color-accent); border-radius: 50%; animation: spin .8s linear infinite; }
  @keyframes spin { to { transform: rotate(360deg); } }
</style>

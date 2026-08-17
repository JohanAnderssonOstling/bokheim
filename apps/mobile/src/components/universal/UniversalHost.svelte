<script lang="ts">
  import AccountPage from "./AccountPage.svelte";
  import SettingsPage from "./SettingsPage.svelte";
  import StorePage from "./StorePage.svelte";
  import type { Destination } from "../../lib/navigation";
  import type { AccountStatus, AccountStorageUsage, BrowsingPreferencePatch, BrowsingPreferences, LibrarySyncStatus, LibraryTransfers, LocalLibraryStorageUsage, NetworkStatus, ServerLibraryStorageUsage, TransferPreferencePatch, TransferPreferences } from "../../lib/types";

  let { destination, activeLibraryId, libraryName, accountResolved, accountStatusError, account, accountBusy, accountProgress, accountError, accountStorageUsage, librarySyncStatuses, libraryTransfers, localLibraryStorageUsage, serverLibraryStorageUsage, preferences, networkStatus, transferPreferences, settingsError, isWebApp, onrefresh, onretry, onlogin, onlogout, onregister, onresendverification, onverifyemail, onrequestpasswordreset, onresetpassword, onpreferencechange, ontransferpreference, onimported, oncreatelibrary }: {
    destination: Destination; activeLibraryId: string; libraryName: string; accountResolved: boolean; accountStatusError: string; account: AccountStatus; accountBusy: boolean; accountProgress: string; accountError: string; accountStorageUsage: AccountStorageUsage | null; librarySyncStatuses: LibrarySyncStatus[]; libraryTransfers: LibraryTransfers[]; localLibraryStorageUsage: LocalLibraryStorageUsage[]; serverLibraryStorageUsage: ServerLibraryStorageUsage[] | null; preferences: BrowsingPreferences; networkStatus: NetworkStatus; transferPreferences: TransferPreferences; settingsError: string; isWebApp: boolean;
    onrefresh: () => Promise<boolean>; onretry: () => void | Promise<void>; onlogin: (email: string, password: string) => Promise<boolean>; onlogout: () => void | Promise<void>; onregister: (email: string, password: string) => Promise<boolean>; onresendverification: (email: string) => Promise<boolean>; onverifyemail: (email: string, pin: string) => Promise<boolean>; onrequestpasswordreset: (email: string) => Promise<boolean>; onresetpassword: (token: string, password: string) => Promise<boolean>; onpreferencechange: (patch: BrowsingPreferencePatch) => void | Promise<void>; ontransferpreference: (patch: TransferPreferencePatch) => void | Promise<void>; onimported: () => void | Promise<void>; oncreatelibrary: () => void;
  } = $props();
</script>

{#if destination === "account"}
  <AccountPage resolved={accountResolved} statusError={accountStatusError} {account} busy={accountBusy} progress={accountProgress} error={accountError} storageUsage={accountStorageUsage} {librarySyncStatuses} {libraryTransfers} {localLibraryStorageUsage} {serverLibraryStorageUsage} {onrefresh} {onretry} {onlogin} {onlogout} {onregister} {onresendverification} {onverifyemail} {onrequestpasswordreset} {onresetpassword} />
{:else if destination === "settings"}
  <SettingsPage {preferences} {networkStatus} {transferPreferences} serverConnected={account.signed_in && networkStatus.connected && networkStatus.validated !== false} error={settingsError} onchange={onpreferencechange} {ontransferpreference} />
{:else if destination === "store"}
  <StorePage {activeLibraryId} {libraryName} {preferences} {onpreferencechange} {onimported} {oncreatelibrary} />
{/if}

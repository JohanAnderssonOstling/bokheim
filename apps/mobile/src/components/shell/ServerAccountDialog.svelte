<script lang="ts">
  import AccountAccessForms from "../auth/AccountAccessForms.svelte";
  import { ACCOUNT_CONTRACT } from "../../lib/types";
  import type { AccountSessionSummary, AccountStatus, LibrarySyncStatus, LibraryTransfers } from "../../lib/types";

  let {
    account,
    accountBusy,
    accountProgress,
    accountError,
    librarySyncStatuses,
    libraryTransfers,
    accountSessions,
    onlogin,
    onlogout,
    onrevokesession,
    onrevokeothersessions,
    onregister,
    onresendverification,
    onverifyemail,
    onrequestpasswordreset,
    onresetpassword,
  }: {
    account: AccountStatus;
    accountBusy: boolean;
    accountProgress: string;
    accountError: string;
    librarySyncStatuses: LibrarySyncStatus[];
    libraryTransfers: LibraryTransfers[];
    accountSessions: AccountSessionSummary[];
    onlogin: (email: string, password: string) => Promise<boolean>;
    onlogout: () => void | Promise<void>;
    onrevokesession: (sessionId: string) => void | Promise<void>;
    onrevokeothersessions: () => void | Promise<void>;
    onregister: (email: string, password: string) => Promise<boolean>;
    onresendverification: (email: string) => Promise<boolean>;
    onverifyemail: (token: string) => Promise<boolean>;
    onrequestpasswordreset: (email: string) => Promise<boolean>;
    onresetpassword: (token: string, password: string) => Promise<boolean>;
  } = $props();

  function activeUploads(libraryId: string) {
    return libraryTransfers
      .find((group) => group.library_id === libraryId)
      ?.transfers.filter((transfer) => transfer.kind === "upload_book" || transfer.kind === "upload_thumbnail") ?? [];
  }

  function syncStatusText(library: LibrarySyncStatus) {
    const uploads = activeUploads(library.library_id);
    if (uploads.some((transfer) => transfer.state === "retrying")) return "Waiting to retry";
    if (uploads.length) return `${uploads.length} ${uploads.length === 1 ? "file" : "files"} remaining`;
    return ACCOUNT_CONTRACT.sync_states.find((definition) => definition.state === library.state)?.label ?? library.state;
  }

  function syncStateLabel(library: LibrarySyncStatus) {
    return ACCOUNT_CONTRACT.sync_states.find((definition) => definition.state === library.state)?.label ?? library.state;
  }

  function transferKindLabel(kind: string) {
    return kind.split("_").map((part) => part ? `${part[0].toUpperCase()}${part.slice(1)}` : part).join(" ");
  }

  function sessionTime(timestamp: number | null) {
    if (timestamp === null) return "Unknown";
    return new Date(timestamp * 1000).toLocaleString();
  }
</script>

<section class="account-page" aria-label={ACCOUNT_CONTRACT.title}>
  <div class="account-panel">
    <div class="account-content">
      {#if account.signed_in}
        <div class="account-summary">
          <span><strong>{account.email}</strong><small>{account.server_url}</small></span>
          <div class="account-actions">
            <button class="quiet" type="button" disabled={accountBusy} onclick={() => void onlogout()}>{ACCOUNT_CONTRACT.sign_out_label}</button>
          </div>
        </div>

        <div class="server-libraries" aria-label="Active sessions">
          <div class="subheading">
            <span>Sessions</span>
            {#if accountSessions.length > 1}<button class="subheading-action" type="button" disabled={accountBusy} onclick={() => void onrevokeothersessions()}>Sign out others</button>{/if}
          </div>
          {#each accountSessions as session (session.session_id)}
            <div class="session-row">
              <span class="library-copy"><strong>{session.current ? "Current session" : "Other session"}</strong><small>Created {sessionTime(session.created_at)} · expires {sessionTime(session.expires_at)}</small></span>
              <button class="session-action" type="button" disabled={accountBusy} onclick={() => void onrevokesession(session.session_id)}>{session.current ? "Sign out" : "Revoke"}</button>
            </div>
          {:else}
            <p class="no-transfers">No active sessions</p>
          {/each}
        </div>

        <div class="server-libraries" aria-label="Server libraries">
          <div class="subheading"><span>{ACCOUNT_CONTRACT.libraries_section_label}</span></div>
          {#each librarySyncStatuses as library (library.library_id)}
            <div class="library-row">
              <span class="library-copy"><strong>{library.library_name}</strong><small>{syncStatusText(library)}</small></span>
              <span class="sync-membership">{syncStateLabel(library)}</span>
            </div>
          {:else}
            <p class="no-transfers">{ACCOUNT_CONTRACT.no_local_libraries_label}</p>
          {/each}
        </div>
        <div class="server-libraries" aria-label="Active transfers">
          <div class="subheading"><span>{ACCOUNT_CONTRACT.transfers_section_label}</span><small>{libraryTransfers.reduce((count, group) => count + group.transfers.length, 0)}</small></div>
          {#each libraryTransfers as group (group.library_id)}
            {#each group.transfers as transfer (transfer.id)}
              <div class="transfer-row">
                <span class="library-copy"><strong>{transfer.book_content_hash ?? "Library asset"}</strong><small>{group.library_name} · {transferKindLabel(transfer.kind)} · attempt {transfer.attempts}{#if transfer.path} · {transfer.path}{/if}{#if transfer.last_error} · {transfer.last_error}{/if}</small></span>
                <span class:running={transfer.state === "running"} class="sync-membership">{transfer.state}</span>
              </div>
            {/each}
          {:else}
            <p class="no-transfers">{ACCOUNT_CONTRACT.no_active_transfers_label}</p>
          {/each}
        </div>
      {:else}
        <AccountAccessForms busy={accountBusy} progress={accountProgress} error={accountError} {onlogin} {onregister} {onresendverification} {onverifyemail} {onrequestpasswordreset} {onresetpassword} />
      {/if}

      {#if account.signed_in && accountProgress}<p class="account-progress" aria-live="polite">{accountProgress}</p>{/if}
      {#if account.signed_in && accountError}<p class="account-error" role="alert">{accountError}</p>{/if}
    </div>
  </div>
</section>

<style>
  .account-page { width: 100%; min-height: 100%; padding: var(--layout-content-padding); }
  .account-panel { width: min(100%, 57.5rem); margin: 0 auto; padding: 1.125rem; border: 1px solid var(--color-border-color); border-radius: var(--radius-lg); color: var(--color-ink-text); background: var(--color-card-bg); }
  button { font: inherit; }
  button { cursor: pointer; }
  button:disabled { opacity: .55; cursor: default; }
  .account-content { display: grid; gap: 1rem; }
  .account-actions button { min-height: 2.65rem; padding: .55rem .9rem; border: 1px solid var(--color-accent); border-radius: var(--radius-sm); color: var(--color-white-surface); background: var(--color-accent); font-size: .75rem; font-weight: 750; }
  .account-summary { display: grid; gap: .8rem; padding: .75rem; border: 1px solid var(--color-border-color); border-radius: var(--radius-md); background: var(--color-white-surface); }
  .account-summary > span, .library-copy { display: grid; gap: .15rem; min-width: 0; }
  .account-summary small, .library-copy strong { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .account-actions { display: flex; gap: .5rem; }
  .account-actions .quiet { color: var(--color-subtle-text); border-color: var(--color-border-color); background: transparent; }
  .account-progress, .account-error { margin: 0; padding: .2rem .1rem; font-size: .72rem; }
  .account-progress { color: var(--color-subtle-text); }
  .account-error { color: var(--color-danger-text); overflow-wrap: anywhere; }
  .server-libraries { display: grid; gap: .45rem; }
  .subheading { display: flex; align-items: center; justify-content: space-between; color: var(--color-subtle-text); font-size: .72rem; font-weight: 750; }
  .subheading-action, .session-action { min-height: 2rem; padding: .35rem .6rem; border: 1px solid var(--color-border-color); border-radius: var(--radius-sm); color: var(--color-subtle-text); background: transparent; font-size: .68rem; font-weight: 750; }
  small { color: var(--color-subtle-text); font-size: .7rem; }
  strong { font-size: .84rem; }
  .library-row { display: grid; grid-template-columns: minmax(0, 1fr) auto; align-items: center; gap: .75rem; min-height: 3.45rem; padding: .55rem .7rem; border-bottom: 1px solid var(--color-border-color); }
  .library-row:first-of-type { border-top: 1px solid var(--color-border-color); }
  .sync-membership { padding: .25rem .48rem; border-radius: 999px; color: var(--color-accent); background: color-mix(in srgb, var(--color-accent) 10%, transparent); font-size: .66rem; font-weight: 750; }
  .sync-membership.running { color: var(--color-white-surface); background: var(--color-accent); }
  .transfer-row { display: grid; grid-template-columns: minmax(0, 1fr) auto; align-items: center; gap: .75rem; min-height: 3.45rem; padding: .55rem .7rem; border-bottom: 1px solid var(--color-border-color); }
  .session-row { display: grid; grid-template-columns: minmax(0, 1fr) auto; align-items: center; gap: .75rem; min-height: 3.45rem; padding: .55rem .7rem; border-bottom: 1px solid var(--color-border-color); }
  .transfer-row small { overflow-wrap: anywhere; white-space: normal; }
  .no-transfers { margin: 0; padding: .7rem 0; color: var(--color-subtle-text); font-size: .72rem; }
</style>

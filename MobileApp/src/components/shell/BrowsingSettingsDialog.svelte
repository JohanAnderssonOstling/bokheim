<script lang="ts">
  import { networkStatusLabel } from "../../lib/network-status";
  import { BROWSING_PREFERENCES_CONTRACT, type AccountStatus, type BrowsingPreferencePatch, type BrowsingPreferences, type LibrarySyncStatus, type LibraryTransfers, type NetworkStatus, type TransferPreferencePatch, type TransferPreferences } from "../../lib/types";

  let {
    preferences,
    account,
    accountBusy,
    accountProgress,
    accountError,
    librarySyncStatuses,
    libraryTransfers,
    librarySyncBusyId,
    networkStatus,
    transferPreferences,
    onclose,
    onchange,
    onlogin,
    onsync,
    onsynclibrary,
    ontransferpreference,
    onlogout,
  }: {
    preferences: BrowsingPreferences;
    account: AccountStatus;
    accountBusy: boolean;
    accountProgress: string;
    accountError: string;
    librarySyncStatuses: LibrarySyncStatus[];
    libraryTransfers: LibraryTransfers[];
    librarySyncBusyId: string;
    networkStatus: NetworkStatus;
    transferPreferences: TransferPreferences;
    onclose: () => void;
    onchange: (patch: BrowsingPreferencePatch) => void | Promise<void>;
    onlogin: (username: string, password: string) => void | Promise<void>;
    onsync: () => void | Promise<void>;
    onsynclibrary: (libraryId: string) => void | Promise<void>;
    ontransferpreference: (patch: TransferPreferencePatch) => void | Promise<void>;
    onlogout: () => void | Promise<void>;
  } = $props();

  let username = $state("");
  let password = $state("");

  async function submitLogin(event: SubmitEvent) {
    event.preventDefault();
    await onlogin(username.trim(), password);
    if (account.signed_in) password = "";
  }

  function activeUploads(libraryId: string) {
    return libraryTransfers
      .find((group) => group.library_id === libraryId)
      ?.transfers.filter((transfer) => transfer.kind === "upload_book" || transfer.kind === "upload_thumbnail") ?? [];
  }

  function changeUiFontSize(delta: number) {
    const [minimum, maximum] = BROWSING_PREFERENCES_CONTRACT.constraints.ui_font_size_px;
    const value = Math.min(maximum, Math.max(minimum, preferences.ui_font_size_px + delta));
    onchange({ preference: "ui_font_size_px", value });
  }

  function syncStatusText(library: LibrarySyncStatus) {
    const uploads = activeUploads(library.library_id);
    if (uploads.some((transfer) => transfer.state === "retrying")) return "Waiting to retry";
    if (uploads.length) return `${uploads.length} ${uploads.length === 1 ? "file" : "files"} remaining`;
    switch (library.state) {
      case "local_only": return "On this device";
      case "synced": return "On server";
      case "remote_missing": return "Not on server";
      case "sign_in_required": return "Sign in required";
      case "different_account": return "Linked to another account";
    }
  }
</script>

<div class="scrim" role="presentation" onclick={onclose}>
  <dialog class="dialog" open aria-labelledby="browsing-settings-title" onclick={(event) => event.stopPropagation()}>
    <header>
      <div><h2 id="browsing-settings-title">Settings</h2></div>
      <button class="close" type="button" aria-label="Close settings" onclick={onclose}>×</button>
    </header>
    <div class="settings-sections">
      <fieldset class="account-settings">
        <legend>Server account</legend>
        {#if account.signed_in}
          <div class="account-summary">
            <span><strong>{account.username}</strong><small>{account.server_url}</small></span>
            <div class="account-actions">
              <button type="button" disabled={accountBusy} onclick={() => void onsync()}>Sync now</button>
              <button class="quiet" type="button" disabled={accountBusy} onclick={() => void onlogout()}>Log out</button>
            </div>
          </div>
          <div class="network-policy">
            <div class="network-summary">
              <span>{networkStatusLabel(networkStatus)}</span>
              {#if networkStatus.metered === true}<small>Metered</small>{/if}
              {#if networkStatus.validated === false && networkStatus.connected}<small>No internet access</small>{/if}
            </div>
            <label class="switch-row">
              <span><strong>Background transfers on metered connections</strong><small>Manual uploads and downloads always continue.</small></span>
              <input
                type="checkbox"
                checked={transferPreferences.allow_background_on_metered}
                onchange={(event) => void ontransferpreference({ preference: "allow_background_on_metered", value: event.currentTarget.checked })}
              />
            </label>
          </div>
          <div class="server-libraries" aria-label="Server libraries">
            <div class="subheading"><span>Libraries on this device</span><small>{librarySyncStatuses.length}</small></div>
            {#each librarySyncStatuses as library (library.library_id)}
              {@const uploads = activeUploads(library.library_id)}
              {@const busy = librarySyncBusyId === library.library_id || uploads.length > 0}
              <div class="library-row">
                <span class="library-copy"><strong>{library.library_name}</strong><small>{syncStatusText(library)}</small></span>
                <button
                  class="library-sync"
                  type="button"
                  disabled={accountBusy || !!librarySyncBusyId || library.state === "different_account" || library.state === "sign_in_required"}
                  onclick={() => void onsynclibrary(library.library_id)}
                >
                  {#if busy}<span class="activity" aria-hidden="true"></span>{/if}
                  {librarySyncBusyId === library.library_id ? "Working…" : library.state === "synced" ? "Sync" : "Upload and sync"}
                </button>
              </div>
            {/each}
          </div>
        {:else}
          <form class="login-form" onsubmit={submitLogin}>
            <p class="server-origin">https://api.bokheim.se</p>
            <label class="text-field"><span>Username</span><input required bind:value={username} autocomplete="username" /></label>
            <label class="text-field"><span>Password</span><input type="password" required bind:value={password} autocomplete="current-password" /></label>
            <button class="primary" type="submit" disabled={accountBusy}>Log in</button>
          </form>
        {/if}
        {#if accountProgress}<p class="account-progress" aria-live="polite">{accountProgress}</p>{/if}
        {#if accountError}<p class="account-error" role="alert">{accountError}</p>{/if}
      </fieldset>
      <fieldset>
        <legend>Application theme</legend>
        {#each BROWSING_PREFERENCES_CONTRACT.options.themes as option}
          <label class:active={preferences.theme === option.value}>
            <input type="radio" name="browsing-theme" value={option.value} checked={preferences.theme === option.value} onchange={() => onchange({ preference: "theme", value: option.value })} />
            <span><strong>{option.label}</strong><small>{option.description}</small></span>
          </label>
        {/each}
      </fieldset>
      <div class="font-setting" role="group" aria-label="Interface font">
        <span class="font-label">Interface font</span>
        <div class="font-buttons">
          {#each BROWSING_PREFERENCES_CONTRACT.options.fonts as option}
            <button type="button" class:active={preferences.interface_font === option.value} aria-pressed={preferences.interface_font === option.value} onclick={() => onchange({ preference: "interface_font", value: option.value })}>{option.font_name}</button>
          {/each}
        </div>
      </div>
      <div class="font-setting" role="group" aria-label="Heading font">
        <span class="font-label">Heading font</span>
        <div class="font-buttons">
          {#each BROWSING_PREFERENCES_CONTRACT.options.fonts as option}
            <button type="button" class:active={preferences.heading_font === option.value} aria-pressed={preferences.heading_font === option.value} onclick={() => onchange({ preference: "heading_font", value: option.value })}>{option.font_name}</button>
          {/each}
        </div>
      </div>
      <div class="font-setting" role="group" aria-label="UI font size">
        <span class="font-label">UI font size</span>
        <div class="font-size-stepper">
          <button type="button" aria-label="Decrease UI font size" disabled={preferences.ui_font_size_px <= BROWSING_PREFERENCES_CONTRACT.constraints.ui_font_size_px[0]} onclick={() => changeUiFontSize(-1)}>−</button>
          <output aria-live="polite">{preferences.ui_font_size_px}px</output>
          <button type="button" aria-label="Increase UI font size" disabled={preferences.ui_font_size_px >= BROWSING_PREFERENCES_CONTRACT.constraints.ui_font_size_px[1]} onclick={() => changeUiFontSize(1)}>+</button>
        </div>
      </div>
    </div>
  </dialog>
</div>

<style>
  .scrim { position: fixed; z-index: 80; inset: 0; display: grid; place-items: end center; padding: 1rem; padding-bottom: max(1rem, env(safe-area-inset-bottom)); background: var(--color-overlay-scrim); }
  .dialog { position: relative; width: min(100%, 32rem); max-height: min(90dvh, 48rem); margin: 0; padding: 1rem; overflow: hidden; border: 1px solid var(--color-border-color); border-radius: var(--radius-lg); color: var(--color-ink-text); background: var(--color-card-bg); box-shadow: 0 1.5rem 4rem var(--color-box-shadow); }
  header { display: flex; align-items: start; justify-content: space-between; gap: 1rem; margin-bottom: 1rem; }
  h2 { margin: 0; font: 500 1.35rem var(--font-heading-family); }
  button { font: inherit; }
  .close { display: grid; place-items: center; width: 2.4rem; height: 2.4rem; padding: 0 0 .18rem; border: 0; border-radius: var(--radius-sm); color: var(--color-subtle-text); background: var(--color-hover-bg); font-size: 1.5rem; }
  .settings-sections { display: grid; gap: 1rem; max-height: calc(min(90dvh, 48rem) - 5rem); overflow-y: auto; }
  fieldset { display: grid; gap: .55rem; margin: 0; padding: 0; border: 0; }
  .font-setting { display: grid; grid-template-columns: minmax(6rem, 1fr) auto; align-items: center; gap: .75rem; }
  .font-label { color: var(--color-subtle-text); font-size: .75rem; font-weight: 750; }
  .font-buttons { display: grid; grid-template-columns: repeat(2, minmax(5.5rem, 1fr)); gap: 1px; overflow: hidden; border: 1px solid var(--color-border-color); border-radius: var(--radius-sm); background: var(--color-border-color); }
  .font-buttons button { min-height: 2.25rem; padding: .4rem .65rem; border: 0; border-radius: 0; color: var(--color-ink-text); background: var(--color-white-surface); font-size: .76rem; font-weight: 700; cursor: pointer; }
  .font-buttons button.active { color: var(--color-white-surface); background: var(--color-accent); }
  .font-size-stepper { display: grid; grid-template-columns: 2.25rem 3.6rem 2.25rem; gap: 1px; overflow: hidden; border: 1px solid var(--color-border-color); border-radius: var(--radius-sm); background: var(--color-border-color); }
  .font-size-stepper button, .font-size-stepper output { display: grid; min-height: 2.25rem; place-items: center; border: 0; border-radius: 0; color: var(--color-ink-text); background: var(--color-white-surface); font-size: .76rem; font-weight: 700; }
  .font-size-stepper button { cursor: pointer; }
  legend { margin-bottom: .55rem; color: var(--color-subtle-text); font-size: .75rem; font-weight: 750; }
  fieldset > label { display: grid; grid-template-columns: auto minmax(0, 1fr); align-items: center; gap: .75rem; min-height: 3.75rem; padding: .65rem .75rem; border: 1px solid var(--color-border-color); border-radius: var(--radius-md); background: var(--color-white-surface); }
  label.active { border-color: var(--color-accent); background: var(--color-active-highlight); }
  input { accent-color: var(--color-accent); }
  label span { display: grid; gap: .15rem; }
  strong { font-size: .84rem; }
  small { color: var(--color-subtle-text); font-size: .7rem; }
  .login-form { display: grid; gap: .55rem; }
  .server-origin { margin: 0; color: var(--color-subtle-text); font-size: .72rem; }
  .text-field { display: grid; grid-template-columns: 5.5rem minmax(0, 1fr); align-items: center; min-height: 2.8rem; padding: 0 .7rem; border: 1px solid var(--color-border-color); border-radius: var(--radius-sm); background: var(--color-white-surface); }
  .text-field span { color: var(--color-subtle-text); font-size: .72rem; font-weight: 700; }
  .text-field input { min-width: 0; height: 2.7rem; border: 0; outline: 0; color: inherit; background: transparent; font: inherit; font-size: .8rem; }
  .text-field:focus-within { border-color: var(--color-accent); }
  .primary, .account-actions button { min-height: 2.65rem; padding: .55rem .9rem; border: 1px solid var(--color-accent); border-radius: var(--radius-sm); color: var(--color-white-surface); background: var(--color-accent); font-size: .75rem; font-weight: 750; }
  button:disabled { opacity: .55; }
  .account-summary { display: grid; gap: .8rem; padding: .75rem; border: 1px solid var(--color-border-color); border-radius: var(--radius-md); background: var(--color-white-surface); }
  .account-summary > span { display: grid; gap: .15rem; min-width: 0; }
  .account-summary small { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .account-actions { display: flex; gap: .5rem; }
  .account-actions .quiet { color: var(--color-subtle-text); border-color: var(--color-border-color); background: transparent; }
  .account-progress, .account-error { margin: 0; padding: .2rem .1rem; font-size: .72rem; }
  .account-progress { color: var(--color-subtle-text); }
  .account-error { color: var(--color-danger-text); overflow-wrap: anywhere; }
  .network-policy, .server-libraries { display: grid; gap: .45rem; }
  .network-summary { display: flex; align-items: baseline; gap: .5rem; padding: .1rem .05rem; color: var(--color-subtle-text); font-size: .72rem; }
  .network-summary > span { color: var(--color-ink-text); font-weight: 750; }
  .network-summary small + small::before { content: "·"; margin-right: .5rem; }
  .switch-row { display: grid; grid-template-columns: minmax(0, 1fr) auto; align-items: center; gap: .8rem; min-height: 3.45rem; padding: .55rem .7rem; border: 1px solid var(--color-border-color); border-radius: var(--radius-sm); background: var(--color-white-surface); }
  .switch-row > span, .library-copy { display: grid; gap: .12rem; min-width: 0; }
  .switch-row input { width: 1.05rem; height: 1.05rem; }
  .subheading { display: flex; align-items: center; justify-content: space-between; color: var(--color-subtle-text); font-size: .72rem; font-weight: 750; }
  .library-row { display: grid; grid-template-columns: minmax(0, 1fr) auto; align-items: center; gap: .75rem; min-height: 3.45rem; padding: .55rem .7rem; border-bottom: 1px solid var(--color-border-color); }
  .library-row:first-of-type { border-top: 1px solid var(--color-border-color); }
  .library-copy strong { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .library-sync { display: inline-flex; align-items: center; justify-content: center; gap: .4rem; min-width: 5.7rem; min-height: 2.3rem; padding: .4rem .65rem; border: 1px solid var(--color-border-color); border-radius: var(--radius-sm); color: var(--color-accent); background: transparent; font-size: .7rem; font-weight: 750; }
  .activity { width: .85rem; height: .85rem; border: 1.5px solid color-mix(in srgb, currentColor 28%, transparent); border-top-color: currentColor; border-radius: 50%; animation: spin .8s linear infinite; }
  @keyframes spin { to { transform: rotate(360deg); } }
  @media (min-width: 45rem) { .scrim { place-items: center; } }
</style>

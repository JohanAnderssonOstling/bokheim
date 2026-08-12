<script lang="ts">
  import { networkStatusLabel } from "../../lib/network-status";
  import { BROWSING_PREFERENCES_CONTRACT, SETTINGS_CONTRACT, type BrowsingPreferencePatch, type BrowsingPreferences, type NetworkStatus, type TransferPreferencePatch, type TransferPreferences } from "../../lib/types";

  let {
    preferences,
    networkStatus,
    transferPreferences,
    serverConnected,
    error,
    onchange,
    ontransferpreference,
  }: {
    preferences: BrowsingPreferences;
    networkStatus: NetworkStatus;
    transferPreferences: TransferPreferences;
    serverConnected: boolean;
    error: string;
    onchange: (patch: BrowsingPreferencePatch) => void | Promise<void>;
    ontransferpreference: (patch: TransferPreferencePatch) => void | Promise<void>;
  } = $props();

  const [minimumUiFontSize, maximumUiFontSize] = BROWSING_PREFERENCES_CONTRACT.constraints.ui_font_size_px;
  const catalogLanguageValues = BROWSING_PREFERENCES_CONTRACT.options.catalog_languages.map((option) => option.value);
  let autoDownloadLimit = $derived(transferPreferences.auto_download_limit_mb ?? 0);

  function catalogLanguageIncluded(language: string): boolean {
    return preferences.included_catalog_languages.length === 0 || preferences.included_catalog_languages.includes(language);
  }

  function toggleCatalogLanguage(language: string, included: boolean) {
    const current = preferences.included_catalog_languages.length === 0 ? [...catalogLanguageValues] : [...preferences.included_catalog_languages];
    const next = included ? [...new Set([...current, language])] : current.filter((value) => value !== language);
    return onchange({ preference: "included_catalog_languages", value: next.length === catalogLanguageValues.length ? [] : next });
  }

</script>

<section class="settings-page" aria-label={SETTINGS_CONTRACT.title}>
  <div class="settings-panel">
    <div class="settings-sections">
      <fieldset>
        <legend>{SETTINGS_CONTRACT.appearance_section_label}</legend>
        <span class="setting-label">Light or dark</span>
        {#each BROWSING_PREFERENCES_CONTRACT.options.appearances as option}
          <label class:active={preferences.appearance === option.value}>
            <input type="radio" name="browsing-appearance" value={option.value} checked={preferences.appearance === option.value} onchange={() => onchange({ preference: "appearance", value: option.value })} />
            <span><strong>{option.label}</strong><small>{option.description}</small></span>
          </label>
        {/each}
        <span class="setting-label">{SETTINGS_CONTRACT.application_theme_label}</span>
        {#each BROWSING_PREFERENCES_CONTRACT.options.themes as option}
          <label class:active={preferences.theme === option.value}>
            <input type="radio" name="browsing-theme" value={option.value} checked={preferences.theme === option.value} onchange={() => onchange({ preference: "theme", value: option.value })} />
            <span><strong>{option.label}</strong><small>{option.description}</small></span>
          </label>
        {/each}
      </fieldset>
      <div class="font-setting" role="group" aria-label={SETTINGS_CONTRACT.interface_font_label}>
        <span class="font-label">{SETTINGS_CONTRACT.interface_font_label}</span>
        <div class="font-buttons">
          {#each BROWSING_PREFERENCES_CONTRACT.options.fonts as option}
            <button type="button" class:active={preferences.interface_font === option.value} aria-pressed={preferences.interface_font === option.value} onclick={() => onchange({ preference: "interface_font", value: option.value })}>{option.font_name}</button>
          {/each}
        </div>
      </div>
      <div class="font-setting" role="group" aria-label={SETTINGS_CONTRACT.ui_font_size_label}>
        <span class="font-label">{SETTINGS_CONTRACT.ui_font_size_label}</span>
        <div class="stepper">
          <button type="button" aria-label="Decrease UI font size" disabled={preferences.ui_font_size_px <= minimumUiFontSize} onclick={() => onchange({ preference: "ui_font_size_px", value: Math.max(minimumUiFontSize, preferences.ui_font_size_px - 1) })}>−</button>
          <output>{preferences.ui_font_size_px}px</output>
          <button type="button" aria-label="Increase UI font size" disabled={preferences.ui_font_size_px >= maximumUiFontSize} onclick={() => onchange({ preference: "ui_font_size_px", value: Math.min(maximumUiFontSize, preferences.ui_font_size_px + 1) })}>+</button>
        </div>
      </div>
      <fieldset>
        <legend>{SETTINGS_CONTRACT.catalogues_section_label}</legend>
        <span class="setting-label">{SETTINGS_CONTRACT.included_catalog_languages_label}</span>
        <small>{SETTINGS_CONTRACT.included_catalog_languages_description}</small>
        <label class:active={preferences.included_catalog_languages.length === 0}>
          <input type="checkbox" checked={preferences.included_catalog_languages.length === 0} onchange={() => onchange({ preference: "included_catalog_languages", value: [] })} />
          <span><strong>{SETTINGS_CONTRACT.all_catalog_languages_label}</strong></span>
        </label>
        {#each BROWSING_PREFERENCES_CONTRACT.options.catalog_languages as option}
          <label class:active={catalogLanguageIncluded(option.value)}>
            <input type="checkbox" checked={catalogLanguageIncluded(option.value)} onchange={(event) => toggleCatalogLanguage(option.value, event.currentTarget.checked)} />
            <span><strong>{option.label}</strong></span>
          </label>
        {/each}
      </fieldset>
      <fieldset>
        <legend>{SETTINGS_CONTRACT.transfers_section_label}</legend>
        <div class="network-policy">
          <div class="network-summary">
            <span>{SETTINGS_CONTRACT.current_network_label}: {networkStatusLabel(networkStatus)}</span>
            {#if networkStatus.metered === true}<small>Metered</small>{/if}
            {#if networkStatus.validated === false && networkStatus.connected}<small>No internet access</small>{/if}
          </div>
          <label class="switch-row">
            <span><strong>{SETTINGS_CONTRACT.background_metered_label}</strong><small>{SETTINGS_CONTRACT.background_metered_description}</small></span>
            <input
              type="checkbox"
              checked={transferPreferences.allow_background_on_metered}
              onchange={(event) => void ontransferpreference({ preference: "allow_background_on_metered", value: event.currentTarget.checked })}
            />
          </label>
          <div class="switch-row">
            <span><strong>{SETTINGS_CONTRACT.auto_download_label}</strong><small>{SETTINGS_CONTRACT.auto_download_description}</small></span>
            <div class="stepper">
              <button type="button" aria-label="Decrease automatic download limit" disabled={autoDownloadLimit === 0} onclick={() => ontransferpreference({ preference: "auto_download_limit_mb", value: autoDownloadLimit > 1 ? autoDownloadLimit - 1 : null })}>−</button>
              <output>{autoDownloadLimit === 0 ? "Off" : `${autoDownloadLimit} MB`}</output>
              <button type="button" aria-label="Increase automatic download limit" onclick={() => ontransferpreference({ preference: "auto_download_limit_mb", value: autoDownloadLimit + 1 })}>+</button>
            </div>
          </div>
        </div>
      </fieldset>
      {#if serverConnected}
        <fieldset>
          <legend>{SETTINGS_CONTRACT.library_section_label}</legend>
          <label class="switch-row">
            <span><strong>{SETTINGS_CONTRACT.show_unavailable_books_label}</strong><small>Include server books whose local file is currently unavailable.</small></span>
            <input type="checkbox" checked={preferences.show_unavailable_books} onchange={(event) => onchange({ preference: "show_unavailable_books", value: event.currentTarget.checked })} />
          </label>
        </fieldset>
      {/if}
      {#if error}<p class="settings-error" role="alert">{error}</p>{/if}
    </div>
  </div>
</section>

<style>
  .settings-page { width: 100%; min-height: 100%; padding: var(--layout-content-padding); }
  .settings-panel { width: min(100%, 36.25rem); margin: 0 auto; padding: 1.125rem; border: 1px solid var(--color-border); border-radius: var(--radius-lg); color: var(--color-ink-text); background: var(--color-raised-bg); }
  button { font: inherit; }
  .settings-sections { display: grid; gap: 1rem; }
  fieldset { display: grid; gap: .55rem; margin: 0; padding: 0; border: 0; }
  .font-setting { display: grid; grid-template-columns: minmax(6rem, 1fr) auto; align-items: center; gap: .75rem; }
  .font-label { color: var(--color-subtle-text); font-size: .75rem; font-weight: 750; }
  .setting-label { color: var(--color-subtle-text); font-size: .75rem; font-weight: 750; }
  .font-buttons { display: grid; grid-template-columns: repeat(auto-fit, minmax(4.5rem, 1fr)); gap: 1px; overflow: hidden; border: 1px solid var(--color-border); border-radius: var(--radius-sm); background: var(--color-border); }
  .font-buttons button { min-height: 2.25rem; padding: .4rem .65rem; border: 0; border-radius: 0; color: var(--color-ink-text); background: var(--color-raised-bg); font-size: .76rem; font-weight: 700; cursor: pointer; }
  .font-buttons button.active { color: var(--color-raised-bg); background: var(--color-accent); }
  .stepper { display: grid; grid-template-columns: 2.25rem minmax(4.5rem, auto) 2.25rem; align-items: center; }
  .stepper button { width: 2.25rem; min-height: 2.25rem; padding: 0; border: 1px solid var(--color-border); color: var(--color-ink-text); background: var(--color-raised-bg); cursor: pointer; }
  .stepper button:first-child { border-radius: var(--radius-sm) 0 0 var(--radius-sm); }
  .stepper button:last-child { border-radius: 0 var(--radius-sm) var(--radius-sm) 0; }
  .stepper output { min-height: 2.25rem; display: flex; align-items: center; justify-content: center; border-block: 1px solid var(--color-border); color: var(--color-subtle-text); background: var(--color-raised-bg); font-size: .75rem; font-weight: 700; white-space: nowrap; }
  legend { margin-bottom: .55rem; color: var(--color-subtle-text); font-size: .75rem; font-weight: 750; }
  fieldset > label { display: grid; grid-template-columns: auto minmax(0, 1fr); align-items: center; gap: .75rem; min-height: 3.75rem; padding: .65rem .75rem; border: 1px solid var(--color-border); border-radius: var(--radius-md); background: var(--color-raised-bg); }
  label.active { border-color: var(--color-accent); background: var(--color-active-highlight); }
  input { accent-color: var(--color-accent); }
  label span { display: grid; gap: .15rem; }
  strong { font-size: .84rem; }
  small { color: var(--color-subtle-text); font-size: .7rem; }
  button:disabled { opacity: .55; }
  .network-policy { display: grid; gap: .45rem; }
  .network-summary { display: flex; align-items: baseline; gap: .5rem; padding: .1rem .05rem; color: var(--color-subtle-text); font-size: .72rem; }
  .network-summary > span { color: var(--color-ink-text); font-weight: 750; }
  .network-summary small + small::before { content: "·"; margin-right: .5rem; }
  .settings-error { margin: 0; color: var(--color-danger-text); font-size: .72rem; overflow-wrap: anywhere; }
  .switch-row { display: grid; grid-template-columns: minmax(0, 1fr) auto; align-items: center; gap: .8rem; min-height: 3.45rem; padding: .55rem .7rem; border: 1px solid var(--color-border); border-radius: var(--radius-sm); background: var(--color-raised-bg); }
  .switch-row > span { display: grid; gap: .12rem; min-width: 0; }
  .switch-row input { width: 1.05rem; height: 1.05rem; }
</style>

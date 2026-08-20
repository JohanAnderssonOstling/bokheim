<script lang="ts">
  import { SETTINGS_CONTRACT } from "../../lib/shared/types";
  import AppearanceSettingsSection from "./components/AppearanceSettingsSection.svelte";
  import { createSettingsPage } from "./setting.svelte";

  const settings = createSettingsPage();
</script>

<svelte:head>
  <title>Settings · Bokheim</title>
</svelte:head>

<section
  class="settings-page"
  aria-label={SETTINGS_CONTRACT.title}
>
  <div class="settings-panel">
    <div class="settings-sections">
      <AppearanceSettingsSection
        preferences={settings.preferences}
        onchange={settings.savePreference}
      />
      {#if settings.error}
        <p
          class="settings-error"
          role="alert"
        >
          {settings.error}
        </p>
      {/if}
    </div>
  </div>
</section>

<style>
  .settings-page {
    width: 100%;
    min-height: 100%;
    padding: var(--layout-content-padding);
  }
  .settings-panel {
    width: min(100%, 36.25rem);
    margin: 0 auto;
    padding: 1.125rem;
    border: 1px solid var(--color-border);
    border-radius: 0;
    color: var(--color-ink-text);
    background: var(--color-raised-bg);
  }
  .settings-sections {
    display: grid;
    gap: 1rem;
  }
  .settings-error {
    margin: 0;
    color: var(--color-danger-text);
    font-size: 0.72rem;
    overflow-wrap: anywhere;
  }
</style>

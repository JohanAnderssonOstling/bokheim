<script lang="ts">
  import {
    BROWSING_PREFERENCES_CONTRACT,
    SETTINGS_CONTRACT,
    type BrowsingPreferencePatch,
    type BrowsingPreferences,
  } from "../../../lib/shared/types";
  import SettingsSection from "./SettingsSection.svelte";
  import SettingsButtonGroup from "./SettingsButtonGroup.svelte";
  import SettingStepper from "../../../components/shared/SettingStepper.svelte";

  let {
    preferences,
    onchange,
  }: {
    preferences: BrowsingPreferences;
    onchange: (patch: BrowsingPreferencePatch) => void | Promise<void>;
  } = $props();

  const [minimumUiFontSize, maximumUiFontSize] =
    BROWSING_PREFERENCES_CONTRACT.constraints.ui_font_size_px;
  const appearanceOptions = BROWSING_PREFERENCES_CONTRACT.options.appearances.filter(
    (option) => option.value !== "system",
  );
</script>

<SettingsSection>
  <SettingsButtonGroup
    label="Light or dark"
    options={appearanceOptions}
    value={preferences.appearance}
    onselect={(value) =>
      onchange({
        preference: "appearance",
        value: value as BrowsingPreferences["appearance"],
      })}
  />
  <SettingsButtonGroup
    label={SETTINGS_CONTRACT.application_theme_label}
    options={BROWSING_PREFERENCES_CONTRACT.options.themes}
    value={preferences.theme}
    onselect={(value) =>
      onchange({ preference: "theme", value: value as BrowsingPreferences["theme"] })}
  />
  <SettingsButtonGroup
    label={SETTINGS_CONTRACT.interface_font_label}
    options={BROWSING_PREFERENCES_CONTRACT.options.fonts}
    value={preferences.interface_font}
    onselect={(value) =>
      onchange({
        preference: "interface_font",
        value: value as BrowsingPreferences["interface_font"],
      })}
  />
  <SettingStepper
    label={SETTINGS_CONTRACT.ui_font_size_label}
    value={`${preferences.ui_font_size_px}px`}
    decreaseLabel="Decrease UI font size"
    increaseLabel="Increase UI font size"
    decreaseDisabled={preferences.ui_font_size_px <= minimumUiFontSize}
    increaseDisabled={preferences.ui_font_size_px >= maximumUiFontSize}
    ondecrease={() =>
      onchange({
        preference: "ui_font_size_px",
        value: Math.max(minimumUiFontSize, preferences.ui_font_size_px - 1),
      })}
    onincrease={() =>
      onchange({
        preference: "ui_font_size_px",
        value: Math.min(maximumUiFontSize, preferences.ui_font_size_px + 1),
      })}
  />
</SettingsSection>

<style>
</style>

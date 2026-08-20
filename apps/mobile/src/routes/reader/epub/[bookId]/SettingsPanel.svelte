<script lang="ts">
  import PanelShell from "../../../../components/shared/readers/panels/PanelShell.svelte";
  import SettingStepper from "../../../../components/shared/SettingStepper.svelte";
  import SettingsButtonGroup from "../../../settings/components/SettingsButtonGroup.svelte";



  import {
    READER_PREFERENCES_CONTRACT,
    type ReaderFontFamily,
    type ReaderPreferences,
    type ReaderTextAlignment,
  } from "../../../../lib/shared/types";
  import { readerPreferenceConstraints } from "./reader-preferences.svelte";

  /// The contract no longer caps the column count: how many fit is a question
  /// about the available width, not one the preferences can answer.
  const MINIMUM_COLUMN_COUNT = 1;
  import { useEpubReaderContext } from "./reader-context";
  let reader = useEpubReaderContext();
  let settings = reader.preferences.settings;

  function handleSetFontFamily(value: string) {
    reader.preferences.update("font_family", value as ReaderFontFamily);
  }

  function handleSetTextAlignment(value: string) {
    reader.preferences.update("text_alignment", value as ReaderTextAlignment);
  }
</script>

<PanelShell
  variant="settings"
  ariaLabel="Reading settings"
>
  <div class="settings-section">
    <SettingsButtonGroup
      label="Typeface"
      options={READER_PREFERENCES_CONTRACT.options.font_families}
      value={settings.font_family}
      onselect={handleSetFontFamily}
    />
    <SettingStepper
      label="Font size"
      value={`${settings.font_size_px.toFixed(0)}px`}
      decreaseLabel="Decrease font size"
      increaseLabel="Increase font size"
      decreaseDisabled={settings.font_size_px <= readerPreferenceConstraints.font_size_px[0]}
      increaseDisabled={settings.font_size_px >= readerPreferenceConstraints.font_size_px[1]}
      ondecrease={() => reader.preferences.adjust("font_size_px", -1)}
      onincrease={() => reader.preferences.adjust("font_size_px", 1)}
    />
    <SettingStepper
      label="Line height"
      value={settings.line_height.toFixed(1)}
      decreaseLabel="Decrease line height"
      increaseLabel="Increase line height"
      decreaseDisabled={settings.line_height <= readerPreferenceConstraints.line_height[0]}
      increaseDisabled={settings.line_height >= readerPreferenceConstraints.line_height[1]}
      ondecrease={() => reader.preferences.adjust("line_height", -0.1, 1)}
      onincrease={() => reader.preferences.adjust("line_height", 0.1, 1)}
    />
    <SettingsButtonGroup
      label="Alignment"
      options={READER_PREFERENCES_CONTRACT.options.text_alignments}
      value={settings.text_alignment}
      onselect={handleSetTextAlignment}
    />
  </div>

  <div class="settings-section">
    <SettingStepper
      label="Column width"
      value={`${settings.max_line_width_px.toFixed(0)}px`}
      decreaseLabel="Decrease column width"
      increaseLabel="Increase column width"
      decreaseDisabled={settings.max_line_width_px <=
        readerPreferenceConstraints.max_line_width_px[0]}
      increaseDisabled={settings.max_line_width_px >=
        readerPreferenceConstraints.max_line_width_px[1]}
      ondecrease={() => reader.preferences.adjust("max_line_width_px", -40)}
      onincrease={() => reader.preferences.adjust("max_line_width_px", 40)}
    />
    <SettingStepper
      label="Fit columns"
      value={String(settings.max_column_count)}
      decreaseLabel="Fit fewer columns"
      increaseLabel="Fit more columns"
      decreaseDisabled={settings.max_column_count <= MINIMUM_COLUMN_COUNT}
      ondecrease={() =>
        reader.preferences.update(
          "max_column_count",
          Math.max(MINIMUM_COLUMN_COUNT, settings.max_column_count - 1),
        )}
      onincrease={() =>
        reader.preferences.update("max_column_count", settings.max_column_count + 1)}
    />
    <SettingStepper
      label="Zoom"
      value={`${settings.zoom.toFixed(1)}×`}
      decreaseLabel="Zoom out"
      increaseLabel="Zoom in"
      decreaseDisabled={settings.zoom <= readerPreferenceConstraints.zoom[0]}
      increaseDisabled={settings.zoom >= readerPreferenceConstraints.zoom[1]}
      ondecrease={() => reader.preferences.adjust("zoom", -0.1, 1)}
      onincrease={() => reader.preferences.adjust("zoom", 0.1, 1)}
    />
  </div>
</PanelShell>

<style>
  .settings-section {
    display: grid;
    gap: 0.5rem;
  }

  .settings-section + .settings-section {
    padding-top: var(--reader-settings-gap);
    border-top: 1px solid var(--chrome-strong);
  }
</style>

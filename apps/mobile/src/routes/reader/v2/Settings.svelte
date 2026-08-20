<!--
  Owns both its trigger and its panel, so opening it is local state and nothing
  about settings passes through the top bar.

  The controls come from the shared settings components. Those take a value and
  a callback and hold nothing themselves, which is exactly why reusing them
  costs this component no coupling: presentation is shared, data flow is not.
-->
<script>
  import { onMount } from "svelte";
  import SquareButton from "../../../components/shared/buttons/SquareButton.svelte";
  import FileIcon from "../../../components/shared/FileIcon.svelte";
  import slidersHorizontal from "@icons/phosphor/sliders-horizontal.svg";
  import SettingStepper from "../../../components/shared/SettingStepper.svelte";
  import SettingsButtonGroup from "../../settings/components/SettingsButtonGroup.svelte";
  import { READER_PREFERENCES_CONTRACT } from "../../../lib/shared/types";
  import { commands } from "../../../lib/app/commands";
  import { settings } from "./settings.svelte";

  let options = READER_PREFERENCES_CONTRACT.options;
  let limits = READER_PREFERENCES_CONTRACT.constraints;

  let open = $state(false);

  function toggle() {
    open = !open;
  }

  // The stored record also carries the PDF reader's preferences; only the
  // typography this panel edits is copied across.
  async function load() {
    try {
      let stored = await commands.getReaderPreferences();
      for (let preference of Object.keys(settings)) settings[preference] = stored[preference];
    } catch {
      // The contract defaults are already in place.
    }
  }

  // Applied first so the page reflows as the button is pressed; the write is
  // one preference at a time, which is the shape the backend takes.
  async function change(preference, value) {
    settings[preference] = value;
    try {
      await commands.updateReaderPreference({ preference, value });
    } catch {
      // The reader keeps the change; the next open reads the stored value.
    }
  }

  onMount(load);

  // Constraint bounds are nullable in the contract, meaning "unbounded".
  function clamp(value, [low, high], decimals) {
    let scale = 10 ** decimals;
    let rounded = Math.round(value * scale) / scale;
    if (low !== null) rounded = Math.max(low, rounded);
    if (high !== null) rounded = Math.min(high, rounded);
    return rounded;
  }

  function atLimit(value, bound) {
    return bound !== null && value === bound;
  }

  function step(preference, delta, bounds, decimals) {
    change(preference, clamp(settings[preference] + delta, bounds, decimals));
  }
</script>

<SquareButton
  active={open}
  label="Reader settings"
  ariaExpanded={open}
  ariaHasPopup="dialog"
  onclick={toggle}
>
  <FileIcon source={slidersHorizontal} />
</SquareButton>

{#if open}
  <div class="settings">
    <SettingsButtonGroup
      label="Typeface"
      options={options.font_families}
      value={settings.font_family}
      onselect={(value) => change("font_family", value)}
    />

    <SettingStepper
      label="Font size"
      value={`${settings.font_size_px}px`}
      decreaseLabel="Decrease font size"
      increaseLabel="Increase font size"
      decreaseDisabled={atLimit(settings.font_size_px, limits.font_size_px[0])}
      increaseDisabled={atLimit(settings.font_size_px, limits.font_size_px[1])}
      ondecrease={() => step("font_size_px", -1, limits.font_size_px, 0)}
      onincrease={() => step("font_size_px", 1, limits.font_size_px, 0)}
    />

    <SettingStepper
      label="Line height"
      value={settings.line_height.toFixed(1)}
      decreaseLabel="Decrease line height"
      increaseLabel="Increase line height"
      decreaseDisabled={atLimit(settings.line_height, limits.line_height[0])}
      increaseDisabled={atLimit(settings.line_height, limits.line_height[1])}
      ondecrease={() => step("line_height", -0.1, limits.line_height, 1)}
      onincrease={() => step("line_height", 0.1, limits.line_height, 1)}
    />

    <!-- Column width, not column count: how many fit follows from it. -->
    <SettingStepper
      label="Column width"
      value={`${settings.max_line_width_px}px`}
      decreaseLabel="Narrower columns"
      increaseLabel="Wider columns"
      decreaseDisabled={atLimit(settings.max_line_width_px, limits.max_line_width_px[0])}
      increaseDisabled={atLimit(settings.max_line_width_px, limits.max_line_width_px[1])}
      ondecrease={() => step("max_line_width_px", -20, limits.max_line_width_px, 0)}
      onincrease={() => step("max_line_width_px", 20, limits.max_line_width_px, 0)}
    />

    <SettingsButtonGroup
      label="Alignment"
      options={options.text_alignments}
      value={settings.text_alignment}
      onselect={(value) => change("text_alignment", value)}
    />

    <label class="settings-toggle">
      <input
        type="checkbox"
        checked={settings.hyphenation}
        onchange={(event) => change("hyphenation", event.currentTarget.checked)}
      />
      Hyphenation
    </label>
  </div>
{/if}

<style>
  .settings {
    position: absolute;
    z-index: 2;
    top: 100%;
    right: 0;
    display: grid;
    gap: 0.75rem;
    padding: 0.75rem;
    background: var(--color-surface-bg);
    border: 1px solid var(--color-border);
    box-shadow: 0 1rem 3rem color-mix(in srgb, var(--color-ink-text) 24%, transparent);
  }

  .settings-toggle {
    display: flex;
    align-items: center;
    gap: 0.5rem;
  }
</style>

import { BROWSING_PREFERENCES_CONTRACT, type BrowsingPreferences, type UiFontFamily } from "./types";

function fontCssValue(value: UiFontFamily) {
  const option = BROWSING_PREFERENCES_CONTRACT.options.fonts.find((candidate) => candidate.value === value);
  if (!option) throw new Error(`Missing generated UI font metadata for ${value}`);
  return `"${option.font_name.replaceAll('"', '\\"')}", ${option.generic_family}`;
}

export function applyApplicationAppearance(preferences: BrowsingPreferences) {
  const root = document.documentElement;
  root.dataset.applicationTheme = preferences.theme;
  root.style.setProperty("--font-interface-family", fontCssValue(preferences.interface_font));
  root.style.setProperty("--font-heading-family", fontCssValue(preferences.heading_font));
  root.style.fontSize = `${preferences.ui_font_size_px}px`;
}

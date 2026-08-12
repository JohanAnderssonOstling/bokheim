import { BROWSING_PREFERENCES_CONTRACT, type ApplicationAppearance, type BrowsingPreferences, type UiFontFamily } from "./types";

function fontCssValue(value: UiFontFamily) {
  const option = BROWSING_PREFERENCES_CONTRACT.options.fonts.find((candidate) => candidate.value === value);
  if (!option) throw new Error(`Missing generated UI font metadata for ${value}`);
  return `"${option.font_name.replaceAll('"', '\\"')}", ${option.generic_family}`;
}

const DARK_SCHEME_QUERY = "(prefers-color-scheme: dark)";

function systemPrefersDark() {
  return typeof window !== "undefined" && typeof window.matchMedia === "function" && window.matchMedia(DARK_SCHEME_QUERY).matches;
}

/**
 * Resolves `system` against the platform, mirroring `ApplicationAppearance::resolve`
 * on the Rust side. The CSS only ever sees `light` or `dark`, so stylesheets do
 * not have to consult `prefers-color-scheme` themselves and cannot disagree
 * with the setting.
 */
export function resolveAppearance(appearance: ApplicationAppearance): "light" | "dark" {
  if (appearance === "light") return "light";
  if (appearance === "dark") return "dark";
  return systemPrefersDark() ? "dark" : "light";
}

export function applyApplicationAppearance(preferences: BrowsingPreferences) {
  const root = document.documentElement;
  root.dataset.applicationTheme = preferences.theme;
  root.dataset.applicationAppearance = resolveAppearance(preferences.appearance);
  root.style.setProperty("--font-interface-family", fontCssValue(preferences.interface_font));
  root.style.fontSize = `${preferences.ui_font_size_px}px`;
}

/**
 * Re-applies the palette when the system flips between light and dark.
 *
 * Only meaningful while the preference is `system`, but re-applying
 * unconditionally is cheaper than tracking that and cannot go stale. Returns a
 * teardown so callers can drop the listener.
 */
export function observeSystemAppearance(reapply: () => void): () => void {
  if (typeof window === "undefined" || typeof window.matchMedia !== "function") return () => {};
  const query = window.matchMedia(DARK_SCHEME_QUERY);
  const handler = () => reapply();
  query.addEventListener("change", handler);
  return () => query.removeEventListener("change", handler);
}

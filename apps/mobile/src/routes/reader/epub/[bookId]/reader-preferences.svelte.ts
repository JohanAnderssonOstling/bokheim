import { adjustedValue } from "./reader-style";
import { createRendererSync } from "./reader-renderer-sync";
import { READER_PREFERENCES_CONTRACT } from "../../../../lib/shared/types";
import type {
  ApplicationTheme,
  FoliateRenderer,
  ReaderPreferencePatch,
  ReaderPreferences,
} from "../../../../lib/shared/types";

export interface ReaderPreferencesOptions {
  getPreferences: () => ReaderPreferences;
  getApplicationTheme: () => ApplicationTheme;
  getApplicationAppearance: () => "light" | "dark";
  getViewRenderer: () => FoliateRenderer | undefined;
  getFootnoteRenderer: () => FoliateRenderer | undefined;
  onChange: (patch: ReaderPreferencePatch) => void | Promise<void>;
}

/// Keys the contract still publishes bounds for. Column count is not one: how
/// many fit is decided by the available width, not by a preference range.
type AdjustableKey = "max_line_width_px" | "font_size_px" | "zoom" | "line_height";

export const readerPreferenceConstraints = READER_PREFERENCES_CONTRACT.constraints;

/**
 * Owns the reader's live preference values and keeps every foliate renderer
 * (the publication and, separately, the footnote sheet) in sync with them.
 *
 * `settings` resets from the `preferences` prop whenever it or the app theme
 * changes; edits made here round-trip back through `onChange` so the parent's
 * copy — and thus this effect — eventually agree again.
 */
export function createReaderPreferences({
  getPreferences,
  getApplicationTheme,
  getApplicationAppearance,
  getViewRenderer,
  getFootnoteRenderer,
  onChange,
}: ReaderPreferencesOptions) {
  let settings = $state<ReaderPreferences>({ ...getPreferences() });
  const rendererSync = createRendererSync();

  function applyToRenderer(renderer: FoliateRenderer | undefined) {
    rendererSync.apply(renderer, {
      preferences: settings,
      applicationTheme: getApplicationTheme(),
      applicationAppearance: getApplicationAppearance(),
    });
  }

  function applyAll() {
    applyToRenderer(getViewRenderer());
    applyToRenderer(getFootnoteRenderer());
  }

  function update<K extends keyof ReaderPreferences>(
    key: K,
    value: ReaderPreferences[K],
  ) {
    settings = { ...settings, [key]: value };
    applyAll();
    void onChange({ preference: key, value } as ReaderPreferencePatch);
  }

  function adjust(key: AdjustableKey, delta: number, decimals = 0) {
    update(
      key,
      adjustedValue(settings[key], delta, readerPreferenceConstraints[key], decimals),
    );
  }

  $effect(() => {
    getApplicationTheme();
    settings = { ...getPreferences() };
    applyAll();
  });

  return {
    get settings() {
      return settings;
    },
    applyToRenderer,
    applyAll,
    update,
    adjust,
  };
}

export type ReaderPreferencesController = ReturnType<typeof createReaderPreferences>;

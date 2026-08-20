import type {
  ApplicationTheme,
  FoliateRenderer,
  ReaderPreferences,
} from "../../../../lib/shared/types";
import { readerStyles } from "./reader-style";

/// Foliate's own margin handling is bypassed; page padding comes from the styles.
const READER_MARGIN_PX = 0;
/// Column gap as a percentage of the column width, matching foliate's `gap` unit.
const PAGE_GAP_PERCENT = 7;
/// Upper bound on page height so long viewports do not stretch a single column.
const MAX_BLOCK_SIZE_PX = 1440;

export interface RendererPresentation {
  preferences: ReaderPreferences;
  applicationTheme: ApplicationTheme;
  applicationAppearance: "light" | "dark";
}

/**
 * Pushes reader preferences onto foliate renderers, skipping no-op writes.
 *
 * `setAttribute` re-fires the paginator's `attributeChangedCallback` even when the value is
 * unchanged, and `flow`/`max-inline-size` changes there trigger a full column re-layout of the
 * current chapter. Without diffing, any single preference tweak (e.g. font size) rewrote every
 * attribute and forced that reflow on attributes that hadn't actually changed. Diffing against
 * what was last applied to this renderer means the expensive reflow only fires for an actual
 * column-width/flow change, and `setStyles` (also a reflow, needed for font/theme changes)
 * only fires when the computed CSS text actually differs.
 *
 * The diff is keyed by renderer, so the publication and any footnote view each
 * keep their own record and neither suppresses the other's writes.
 */
export function createRendererSync() {
  const lastApplied = new WeakMap<
    FoliateRenderer,
    { attributes: Record<string, string>; styles: string }
  >();

  function apply(
    renderer: FoliateRenderer | undefined,
    presentation: RendererPresentation,
  ) {
    if (!renderer) return;
    const { preferences, applicationTheme, applicationAppearance } = presentation;
    const attributes: Record<string, string> = {
      flow: preferences.flow,
      "max-inline-size": `${preferences.max_line_width_px}px`,
      "max-column-count": String(preferences.max_column_count),
      zoom: String(preferences.zoom),
      margin: `${READER_MARGIN_PX}px`,
      gap: `${PAGE_GAP_PERCENT}%`,
      "max-block-size": `${MAX_BLOCK_SIZE_PX}px`,
    };
    const styles = readerStyles(preferences, applicationTheme, applicationAppearance);
    const previous = lastApplied.get(renderer);
    for (const [name, value] of Object.entries(attributes)) {
      if (previous?.attributes[name] !== value) renderer.setAttribute(name, value);
    }
    if (previous?.styles !== styles) renderer.setStyles?.(styles);
    lastApplied.set(renderer, { attributes, styles });
  }

  return { apply };
}

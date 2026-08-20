import designTokens from "../../../../../../ui/design-tokens/design_tokens.json";
import type { ApplicationTheme, ReaderPreferences } from "../../../../lib/shared/types";

export function adjustedValue(
  value: number,
  delta: number,
  [minimum, maximum]: readonly [number, number],
  decimals = 0,
) {
  const scale = 10 ** decimals;
  return Math.min(
    maximum,
    Math.max(minimum, Math.round((value + delta) * scale) / scale),
  );
}

export function readerStyles(
  readerPreferences: ReaderPreferences,
  applicationTheme: ApplicationTheme,
  applicationAppearance: "light" | "dark",
) {
  const colors = designTokens.themes[applicationTheme][applicationAppearance];
  const family = designTokens.font_families[readerPreferences.font_family];
  const familyRule = family ? `font-family: ${family} !important;` : "";
  const alignmentRule =
    readerPreferences.text_alignment === "publisher"
      ? ""
      : `text-align: ${readerPreferences.text_alignment} !important;`;
  const hyphens = readerPreferences.hyphenation ? "auto" : "manual";
  const colorScheme = relativeLuminance(colors.surface_bg) < 0.35 ? "dark" : "light";
  return `
      :root { color-scheme: ${colorScheme}; font-size: ${readerPreferences.font_size_px}px !important; }
      html, body { background: ${colors.surface_bg} !important; color: ${colors.ink_text} !important; ${familyRule} }
      p, li, blockquote, dd { line-height: ${readerPreferences.line_height} !important; ${alignmentRule} -webkit-hyphens: ${hyphens}; hyphens: ${hyphens}; }
      a { color: ${colors.accent} !important; }
      img, svg { max-width: 100% !important; max-height: 95vh !important; object-fit: contain; background-color: ${colors.surface_bg}; }
    `;
}

function relativeLuminance(hex: string) {
  const channels = [1, 3, 5]
    .map((offset) => Number.parseInt(hex.slice(offset, offset + 2), 16) / 255)
    .map((value) =>
      value <= 0.03928 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4,
    );
  return channels[0] * 0.2126 + channels[1] * 0.7152 + channels[2] * 0.0722;
}

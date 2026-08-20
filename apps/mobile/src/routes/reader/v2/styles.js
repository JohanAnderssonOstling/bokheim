import designTokens from "../../../../../ui/design-tokens/design_tokens.json";

/**
 * Turns the reader settings into the stylesheet foliate injects into every
 * section document.
 *
 * Publisher styling wins by default, so each rule is only emitted when the
 * reader has actually chosen to override it.
 */
export function readerStyles(settings, theme, appearance) {
  let colors = designTokens.themes[theme][appearance];
  let family = designTokens.font_families[settings.font_family];
  let familyRule = family ? `font-family: ${family} !important;` : "";
  let alignmentRule =
    settings.text_alignment === "publisher"
      ? ""
      : `text-align: ${settings.text_alignment} !important;`;
  let hyphens = settings.hyphenation ? "auto" : "manual";

  // Sections render in their own documents, which inherit nothing from the
  // application: without these the text is the browser's default black and the
  // links its default blue, whatever the theme. The background is stated rather
  // than left transparent so foliate paints the same colour behind the page
  // instead of letting the host show through.
  return `
    :root {
      color-scheme: ${appearance};
      font-size: ${settings.font_size_px}px !important;
    }
    html, body {
      color: ${colors.ink_text} !important;
      background: ${colors.surface_bg} !important;
      ${familyRule}
    }
    a { color: ${colors.accent} !important; }
    p, li, blockquote, dd {
      line-height: ${settings.line_height} !important;
      ${alignmentRule}
      -webkit-hyphens: ${hyphens};
      hyphens: ${hyphens};
    }
  `;
}

import { DEFAULT_READER_PREFERENCES } from "../../../../lib/shared/types";

/**
 * Reader settings, shared by whoever changes them and whoever applies them.
 *
 * The settings overlay writes to this and never touches the view; the reader
 * reads it and never knows an overlay exists.
 *
 * Only the typography half of `ReaderPreferences` is held. The PDF fields in
 * that type belong to the other reader, and the layout fields (flow, columns,
 * line width) are renderer attributes rather than styles.
 */
export const settings = $state({
  font_family: DEFAULT_READER_PREFERENCES.font_family,
  font_size_px: DEFAULT_READER_PREFERENCES.font_size_px,
  line_height: DEFAULT_READER_PREFERENCES.line_height,
  text_alignment: DEFAULT_READER_PREFERENCES.text_alignment,
  hyphenation: DEFAULT_READER_PREFERENCES.hyphenation,
  /// The width of one column. How many fit is derived from it.
  max_line_width_px: DEFAULT_READER_PREFERENCES.max_line_width_px,
});

import {
  READER_NAVIGATION_CONTRACT,
  type ReaderNavigationAction,
} from "../../../../lib/shared/types";
import { isTextEntryTarget, navigationKey } from "./reader-helpers";

export type ReaderKeyIntent =
  | { kind: "navigate"; action: ReaderNavigationAction }
  | { kind: "adjust_font_size"; delta: number }
  | { kind: "toggle_search" }
  | { kind: "cycle_search"; offset: number }
  | { kind: "close" };

export interface ReaderKeyMatch {
  intent: ReaderKeyIntent;
  /// Whether the reader should claim the key from the browser.
  preventDefault: boolean;
}

/**
 * Resolves a key press to the reader action it requests, or null when the key
 * carries no reader meaning.
 *
 * Navigation and font-size keys are suppressed while the user is typing so that
 * arrows and `-` behave normally in the search field and note editor. Escape and
 * the find shortcuts stay live everywhere, since they are how a text field is
 * escaped in the first place.
 *
 * Dispatching the intent is left to the caller: a keymap that reached directly
 * into navigation, preferences, panels and search would couple all four to the
 * key handling.
 */
export function matchReaderKey(event: KeyboardEvent): ReaderKeyMatch | null {
  if (!isTextEntryTarget(event.target)) {
    const key = navigationKey(event.key);
    const shortcut = key
      ? READER_NAVIGATION_CONTRACT.shortcuts.find(
          (candidate) =>
            candidate.key === key &&
            candidate.primary_modifier === (event.ctrlKey || event.metaKey) &&
            candidate.alt === event.altKey &&
            candidate.shift === event.shiftKey,
        )
      : undefined;
    if (shortcut)
      return {
        intent: { kind: "navigate", action: shortcut.action },
        preventDefault: true,
      };

    if (
      event.altKey &&
      !event.ctrlKey &&
      !event.metaKey &&
      (event.key === "+" || event.key === "=" || event.key === "-")
    )
      return {
        intent: { kind: "adjust_font_size", delta: event.key === "-" ? -1 : 1 },
        preventDefault: true,
      };
  }

  if (event.key === "Escape") return { intent: { kind: "close" }, preventDefault: false };

  const primaryModifier = event.ctrlKey || event.metaKey;
  if (primaryModifier && event.key.toLowerCase() === "f")
    return { intent: { kind: "toggle_search" }, preventDefault: true };
  if (primaryModifier && event.key.toLowerCase() === "g")
    return {
      intent: { kind: "cycle_search", offset: event.shiftKey ? -1 : 1 },
      preventDefault: true,
    };

  return null;
}
